#include "Luau/Allocator.h"
#include "Luau/Lexer.h"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <limits>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

namespace
{

constexpr unsigned int kBitsPerByte = 8;

enum class Kind : uint8_t
{
    Eof,
    Whitespace,
    Comment,
    BlockComment,
    Name,
    Number,
    RawString,
    QuotedString,
    Interpolated,
    Attribute,
    AttributeOpen,
    Keyword,
    Operator,
    Error,
    Byte,
};

struct Token
{
    Kind kind;
    uint8_t value;
    uint64_t start;
    uint64_t end;
    uint32_t payload;
};

bool readBytes(void* destination, size_t size)
{
    if (size == 0)
    {
        return true;
    }

    return std::fread(destination, 1, size, stdin) == size;
}

bool readU64(uint64_t& value)
{
    uint8_t bytes[sizeof(value)];
    if (!readBytes(bytes, sizeof(bytes)))
    {
        return false;
    }

    value = 0;
    for (unsigned int i = 0; i < sizeof(bytes); ++i)
    {
        value |= static_cast<uint64_t>(bytes[i]) << (i * kBitsPerByte);
    }

    return true;
}

void writeBytes(const void* source, size_t size)
{
    std::fwrite(source, 1, size, stdout);
}

void writeU16(uint16_t value)
{
    const uint32_t unsignedValue = value;
    const uint8_t bytes[] = {
        static_cast<uint8_t>(unsignedValue),
        static_cast<uint8_t>(unsignedValue >> kBitsPerByte),
    };
    writeBytes(bytes, sizeof(bytes));
}

void writeU32(uint32_t value)
{
    const uint8_t bytes[] = {
        static_cast<uint8_t>(value),
        static_cast<uint8_t>(value >> kBitsPerByte),
        static_cast<uint8_t>(value >> (2 * kBitsPerByte)),
        static_cast<uint8_t>(value >> (3 * kBitsPerByte)),
    };
    writeBytes(bytes, sizeof(bytes));
}

void writeU64(uint64_t value)
{
    uint8_t bytes[sizeof(value)];
    for (unsigned int i = 0; i < sizeof(bytes); ++i)
    {
        bytes[i] = static_cast<uint8_t>(value >> (i * kBitsPerByte));
    }

    writeBytes(bytes, sizeof(bytes));
}

size_t offsetFor(const Luau::Position& position, const std::vector<size_t>& lineStarts, size_t sourceSize)
{
    if (position.line >= lineStarts.size())
    {
        return sourceSize;
    }

    return std::min(sourceSize, lineStarts[position.line] + position.column);
}

uint8_t keywordValue(Luau::Lexeme::Type type)
{
    return static_cast<uint8_t>(type - Luau::Lexeme::Reserved_BEGIN);
}

uint8_t operatorValue(Luau::Lexeme::Type type)
{
    constexpr int kAssignmentOperatorOffset = 9;

    if (type <= Luau::Lexeme::FloorDiv)
    {
        return static_cast<uint8_t>(type - Luau::Lexeme::Equal);
    }

    return static_cast<uint8_t>(type - Luau::Lexeme::AddAssign + kAssignmentOperatorOffset);
}

bool appendToken(std::vector<Token>& result, const Luau::Lexeme& lexeme, const std::vector<size_t>& lineStarts, size_t sourceSize)
{
    const uint64_t start = offsetFor(lexeme.location.begin, lineStarts, sourceSize);
    const uint64_t end = offsetFor(lexeme.location.end, lineStarts, sourceSize);
    const int type = int(lexeme.type);

    if (lexeme.type == Luau::Lexeme::Eof)
    {
        result.push_back({Kind::Eof, 0, start, end, 0});
        return false;
    }

    if (lexeme.type < Luau::Lexeme::Char_END)
    {
        result.push_back({Kind::Byte, static_cast<uint8_t>(type), start, end, 0});
        return true;
    }

    if (lexeme.type >= Luau::Lexeme::Reserved_BEGIN && lexeme.type < Luau::Lexeme::Reserved_END)
    {
        result.push_back({Kind::Keyword, keywordValue(lexeme.type), start, end, 0});
        return true;
    }

    switch (lexeme.type)
    {
    case Luau::Lexeme::Equal:
    case Luau::Lexeme::LessEqual:
    case Luau::Lexeme::GreaterEqual:
    case Luau::Lexeme::NotEqual:
    case Luau::Lexeme::Dot2:
    case Luau::Lexeme::Dot3:
    case Luau::Lexeme::SkinnyArrow:
    case Luau::Lexeme::DoubleColon:
    case Luau::Lexeme::FloorDiv:
    case Luau::Lexeme::AddAssign:
    case Luau::Lexeme::SubAssign:
    case Luau::Lexeme::MulAssign:
    case Luau::Lexeme::DivAssign:
    case Luau::Lexeme::FloorDivAssign:
    case Luau::Lexeme::ModAssign:
    case Luau::Lexeme::PowAssign:
    case Luau::Lexeme::ConcatAssign:
        result.push_back({Kind::Operator, operatorValue(lexeme.type), start, end, 0});
        return true;

    case Luau::Lexeme::InterpStringBegin:
    case Luau::Lexeme::InterpStringMid:
    case Luau::Lexeme::InterpStringEnd:
    case Luau::Lexeme::InterpStringSimple:
        result.push_back({Kind::Interpolated, static_cast<uint8_t>(type - Luau::Lexeme::InterpStringBegin), start, end, 0});
        return true;

    case Luau::Lexeme::RawString:
        result.push_back({Kind::RawString, 0, start, end, 0});
        return true;
    case Luau::Lexeme::QuotedString:
        result.push_back({Kind::QuotedString, 0, start, end, 0});
        return true;
    case Luau::Lexeme::Number:
        result.push_back({Kind::Number, 0, start, end, 0});
        return true;
    case Luau::Lexeme::Name:
        result.push_back({Kind::Name, 0, start, end, 0});
        return true;
    case Luau::Lexeme::Comment:
        result.push_back({Kind::Comment, 0, start, end, 0});
        return true;
    case Luau::Lexeme::BlockComment:
        result.push_back({Kind::BlockComment, 0, start, end, 0});
        return true;
    case Luau::Lexeme::Attribute:
        result.push_back({Kind::Attribute, 0, start, end, 0});
        return true;
    case Luau::Lexeme::AttributeOpen:
        result.push_back({Kind::AttributeOpen, 0, start, end, 0});
        return true;
    case Luau::Lexeme::BrokenString:
        result.push_back({Kind::Error, 0, start, end, 0});
        return true;
    case Luau::Lexeme::BrokenComment:
        result.push_back({Kind::Error, 1, start, end, 0});
        return true;
    case Luau::Lexeme::BrokenUnicode:
        result.push_back({Kind::Error, 2, start, end, lexeme.codepoint});
        return true;
    case Luau::Lexeme::BrokenInterpDoubleBrace:
        result.push_back({Kind::Error, 3, start, end, 0});
        return true;
    case Luau::Lexeme::Error:
        result.push_back({Kind::Error, 4, start, end, 0});
        return true;
    default:
        return true;
    }
}

std::vector<Token> lex(const std::vector<char>& source)
{
    std::vector<size_t> lineStarts = {0};
    for (size_t i = 0; i < source.size(); ++i)
    {
        if (source[i] == '\n')
        {
            lineStarts.push_back(i + 1);
        }
    }

    Luau::Allocator allocator;
    Luau::AstNameTable names(allocator);
    Luau::Lexer lexer(source.data(), source.size(), names);
    std::vector<Token> result;

    while (appendToken(result, lexer.next(), lineStarts, source.size()))
    {
    }

    return result;
}

bool writeResult(const std::vector<Token>& tokens)
{
    if (tokens.size() > std::numeric_limits<uint32_t>::max())
    {
        return false;
    }

    writeU32(static_cast<uint32_t>(tokens.size()));

    for (const Token& token : tokens)
    {
        const uint8_t kind = static_cast<uint8_t>(token.kind);
        writeBytes(&kind, sizeof(kind));
        writeBytes(&token.value, sizeof(token.value));
        writeU16(0);
        writeU64(token.start);
        writeU64(token.end);
        writeU32(token.payload);
    }

    return std::fflush(stdout) == 0;
}

bool setBinaryMode()
{
#ifdef _WIN32
    return _setmode(_fileno(stdin), _O_BINARY) != -1 && _setmode(_fileno(stdout), _O_BINARY) != -1;
#else
    return true;
#endif
}

} // namespace

int run()
{
    if (!setBinaryMode())
    {
        return 1;
    }

    for (;;)
    {
        uint64_t size = 0;
        if (!readU64(size))
        {
            return std::feof(stdin) ? 0 : 1;
        }

        if (size > static_cast<uint64_t>(std::numeric_limits<size_t>::max()))
        {
            return 1;
        }

        std::vector<char> source(static_cast<size_t>(size));
        if (!readBytes(source.data(), source.size()))
        {
            return 1;
        }

        if (!writeResult(lex(source)))
        {
            return 1;
        }
    }
}

int main() noexcept
{
    try
    {
        return run();
    }
    catch (...)
    {
        return 1;
    }
}
