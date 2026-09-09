#include "Luau/Allocator.h"
#include "Luau/Ast.h"
#include "Luau/Lexer.h"
#include "Luau/Parser.h"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <limits>
#include <vector>

LUAU_FASTFLAG(LuauExportValueSyntax)
LUAU_FASTFLAG(LuauIntegerType2)
LUAU_FASTFLAG(DebugLuauIfLocalSyntax)

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

enum class Request : uint8_t
{
    Lex = 0,
    Chunk = 1,
    Expression = 2,
    Type = 3,
};

enum class EventTag : uint8_t
{
    Chunk = 1,
    Block,
    Local,
    LocalFunction,
    Assignment,
    CompoundAssignment,
    CallStatement,
    Return,
    Break,
    Continue,
    Do,
    If,
    While,
    Repeat,
    NumericFor,
    GenericFor,
    FunctionStatement,
    TypeAlias,
    TypeFunctionStatement,
    DeclareGlobal,
    DeclareFunction,
    DeclareExternType,
    Class,
    Export,
    Binding,
    FunctionName,
    ClassMember,
    TypeParameter,
    GenericParameter,
    Attribute,
    Nil,
    Boolean,
    Number,
    String,
    Name,
    Vararg,
    Unary,
    Binary,
    Group,
    IfElse,
    TypeAssertion,
    Interpolated,
    Table,
    TableField,
    TableKeyName,
    TableKeyExpression,
    Call,
    Index,
    Field,
    Function,
    Instantiate,
    TypeName,
    TypeNil,
    TypeTable,
    TypeFunction,
    Typeof,
    TypeOptional,
    TypeUnion,
    TypeIntersection,
    TypeBoolean,
    TypeString,
    TypeNumber,
    TypeGroup,
    TypeField,
    TypeIndexer,
    TypePack,
    TypePackVariadic,
    TypePackGeneric,
};

struct Event
{
    EventTag tag;
    uint8_t flags;
    uint64_t start;
    uint64_t end;
    uint32_t value;
};

struct ParseErrorSpan
{
    uint64_t start;
    uint64_t end;
};

struct ParseOutput
{
    std::vector<Event> events;
    std::vector<ParseErrorSpan> errors;
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

class AstSerializer
{
public:
    AstSerializer(const std::vector<size_t>& lineStarts, size_t sourceSize)
        : lineStarts(lineStarts)
        , sourceSize(sourceSize)
    {
    }

    ParseOutput chunk(Luau::AstStatBlock* root)
    {
        emit(EventTag::Chunk, root->location);
        for (Luau::AstStat* stat : root->body)
            statement(stat);
        return output;
    }

    ParseOutput expression(Luau::AstExpr* root)
    {
        expr(root);
        return output;
    }

    ParseOutput type(Luau::AstType* root)
    {
        typeNode(root);
        return output;
    }

private:
    static constexpr uint8_t kFlagConst = 1;
    static constexpr uint8_t kFlagExported = 2;
    static constexpr uint8_t kFlagOpen = 4;
    static constexpr uint8_t kFlagMethod = 8;
    static constexpr uint8_t kFlagSelf = 16;
    static constexpr uint8_t kFlagVararg = 32;
    static constexpr uint8_t kFlagHasAnnotation = 64;
    static constexpr uint8_t kFlagNamed = 128;

    enum BinaryValue : uint8_t
    {
        kBinaryOr,
        kBinaryAnd,
        kBinaryLess,
        kBinaryLessEqual,
        kBinaryGreater,
        kBinaryGreaterEqual,
        kBinaryEqual,
        kBinaryNotEqual,
        kBinaryAdd,
        kBinarySubtract,
        kBinaryMultiply,
        kBinaryDivide,
        kBinaryFloorDivide,
        kBinaryModulo,
        kBinaryPower,
        kBinaryConcat,
    };

    std::vector<size_t> lineStarts;
    size_t sourceSize;
    ParseOutput output;

    Luau::Location span(size_t start, size_t end) const
    {
        Luau::Position begin(0, 0);
        Luau::Position finish(0, 0);
        size_t line = 0;
        while (line + 1 < lineStarts.size() && lineStarts[line + 1] <= start)
            ++line;
        begin.line = int(line);
        begin.column = int(start - lineStarts[line]);

        line = 0;
        while (line + 1 < lineStarts.size() && lineStarts[line + 1] <= end)
            ++line;
        finish.line = int(line);
        finish.column = int(end - lineStarts[line]);
        return Luau::Location(begin, finish);
    }

    uint64_t offset(const Luau::Position& position) const
    {
        return offsetFor(position, lineStarts, sourceSize);
    }

    void emit(EventTag tag, const Luau::Location& location, uint8_t flags = 0, uint32_t value = 0)
    {
        output.events.push_back({tag, flags, offset(location.begin), offset(location.end), value});
    }

    void emitSpan(EventTag tag, uint64_t start, uint64_t end, uint8_t flags = 0, uint32_t value = 0)
    {
        output.events.push_back({tag, flags, start, end, value});
    }

    static uint32_t unaryValue(Luau::AstExprUnary::Op op)
    {
        switch (op)
        {
        case Luau::AstExprUnary::Op::Minus:
            return 0;
        case Luau::AstExprUnary::Op::Not:
            return 1;
        case Luau::AstExprUnary::Op::Len:
            return 2;
        }
        return 0;
    }

    static uint32_t binaryValue(Luau::AstExprBinary::Op op)
    {
        switch (op)
        {
        case Luau::AstExprBinary::Or:
            return kBinaryOr;
        case Luau::AstExprBinary::And:
            return kBinaryAnd;
        case Luau::AstExprBinary::CompareLt:
            return kBinaryLess;
        case Luau::AstExprBinary::CompareLe:
            return kBinaryLessEqual;
        case Luau::AstExprBinary::CompareGt:
            return kBinaryGreater;
        case Luau::AstExprBinary::CompareGe:
            return kBinaryGreaterEqual;
        case Luau::AstExprBinary::CompareEq:
            return kBinaryEqual;
        case Luau::AstExprBinary::CompareNe:
            return kBinaryNotEqual;
        case Luau::AstExprBinary::Add:
            return kBinaryAdd;
        case Luau::AstExprBinary::Sub:
            return kBinarySubtract;
        case Luau::AstExprBinary::Mul:
            return kBinaryMultiply;
        case Luau::AstExprBinary::Div:
            return kBinaryDivide;
        case Luau::AstExprBinary::FloorDiv:
            return kBinaryFloorDivide;
        case Luau::AstExprBinary::Mod:
            return kBinaryModulo;
        case Luau::AstExprBinary::Pow:
            return kBinaryPower;
        case Luau::AstExprBinary::Concat:
            return kBinaryConcat;
        }
        return kBinaryOr;
    }

    void generic(Luau::AstGenericType* node, bool isPack)
    {
        emit(EventTag::GenericParameter, node->location, isPack ? kFlagVararg : 0);
        if (node->defaultValue)
            typeNode(node->defaultValue);
    }

    void generic(Luau::AstGenericTypePack* node)
    {
        emit(EventTag::GenericParameter, node->location, kFlagVararg);
        if (node->defaultValue)
            pack(node->defaultValue);
    }

    void attribute(Luau::AstAttr* node)
    {
        emit(EventTag::Attribute, node->location, 0, static_cast<uint32_t>(node->type));
        for (Luau::AstExpr* argument : node->args)
            expr(argument);
    }

    void attributes(const Luau::AstArray<Luau::AstAttr*>& nodes)
    {
        for (Luau::AstAttr* node : nodes)
            attribute(node);
    }

    void binding(Luau::AstLocal* node, bool isConst)
    {
        uint8_t flags = isConst ? kFlagConst : 0;
        if (node->annotation)
            flags = static_cast<uint8_t>(flags | kFlagHasAnnotation);
        emit(EventTag::Binding, node->location, flags);
        if (node->annotation)
            typeNode(node->annotation);
    }

    void functionName(Luau::AstExpr* node)
    {
        emit(EventTag::FunctionName, node->location);
        functionNamePart(node);
    }

    void functionNamePart(Luau::AstExpr* node)
    {
        if (auto global = node->as<Luau::AstExprGlobal>())
        {
            emit(EventTag::Name, global->location);
        }
        else if (auto index = node->as<Luau::AstExprIndexName>())
        {
            functionNamePart(index->expr);
            emit(EventTag::Name, index->indexLocation, index->op == ':' ? kFlagMethod : 0);
        }
        else
            expr(node);
    }

    void function(Luau::AstExprFunction* node, uint64_t startOverride = UINT64_MAX)
    {
        uint8_t flags = node->vararg ? kFlagVararg : 0;
        if (node->varargAnnotation)
            flags |= kFlagHasAnnotation;
        if (node->returnAnnotation)
            flags |= kFlagNamed;
        const uint64_t start = startOverride == UINT64_MAX ? offset(node->location.begin) : startOverride;
        emitSpan(EventTag::Function, start, offset(node->location.end), flags);
        attributes(node->attributes);
        for (Luau::AstGenericType* genericNode : node->generics)
            generic(genericNode, false);
        for (Luau::AstGenericTypePack* genericNode : node->genericPacks)
            generic(genericNode);
        for (Luau::AstLocal* argument : node->args)
            binding(argument, argument->isConst);
        if (node->varargAnnotation)
            pack(node->varargAnnotation);
        if (node->returnAnnotation)
            pack(node->returnAnnotation);
        block(node->body);
    }

    void block(Luau::AstStatBlock* node)
    {
        Luau::Location location = node->location;
        if (node->body.size > 0)
            location.begin = node->body.data[0]->location.begin;
        else
            location.begin = location.end;
        emit(EventTag::Block, location);
        for (Luau::AstStat* stat : node->body)
            statement(stat);
    }

    void ifStatement(Luau::AstStatIf* node)
    {
        emit(EventTag::If, node->location, node->conditionIsConst ? kFlagConst : 0);
        for (Luau::AstStatIf* current = node; current;)
        {
            if (current->conditionLocal)
                binding(current->conditionLocal, current->conditionIsConst);
            expr(current->condition);
            block(current->thenbody);

            if (auto nested = current->elsebody ? current->elsebody->as<Luau::AstStatIf>() : nullptr)
            {
                current = nested;
            }
            else
            {
                if (auto elseBlock = current->elsebody ? current->elsebody->as<Luau::AstStatBlock>() : nullptr)
                    block(elseBlock);
                current = nullptr;
            }
        }
    }

    void statement(Luau::AstStat* node)
    {
        if (auto blockNode = node->as<Luau::AstStatBlock>())
        {
            block(blockNode);
        }
        else if (auto ifNode = node->as<Luau::AstStatIf>())
        {
            ifStatement(ifNode);
        }
        else if (auto whileNode = node->as<Luau::AstStatWhile>())
        {
            emit(EventTag::While, whileNode->location);
            expr(whileNode->condition);
            block(whileNode->body);
        }
        else if (auto repeatNode = node->as<Luau::AstStatRepeat>())
        {
            emit(EventTag::Repeat, repeatNode->location);
            block(repeatNode->body);
            expr(repeatNode->condition);
        }
        else if (node->is<Luau::AstStatBreak>())
        {
            emit(EventTag::Break, node->location);
        }
        else if (node->is<Luau::AstStatContinue>())
        {
            emit(EventTag::Continue, node->location);
        }
        else if (auto returnNode = node->as<Luau::AstStatReturn>())
        {
            emit(EventTag::Return, returnNode->location);
            for (Luau::AstExpr* value : returnNode->list)
                expr(value);
        }
        else if (auto exprNode = node->as<Luau::AstStatExpr>())
        {
            emit(EventTag::CallStatement, exprNode->location);
            expr(exprNode->expr);
        }
        else if (auto localNode = node->as<Luau::AstStatLocal>())
        {
            emit(EventTag::Local, localNode->location, localNode->isConst ? kFlagConst : 0);
            for (Luau::AstLocal* variable : localNode->vars)
                binding(variable, localNode->isConst);
            for (Luau::AstExpr* value : localNode->values)
                expr(value);
        }
        else if (auto forNode = node->as<Luau::AstStatFor>())
        {
            emit(EventTag::NumericFor, forNode->location);
            binding(forNode->var, forNode->var->isConst);
            expr(forNode->from);
            expr(forNode->to);
            if (forNode->step)
                expr(forNode->step);
            block(forNode->body);
        }
        else if (auto forInNode = node->as<Luau::AstStatForIn>())
        {
            emit(EventTag::GenericFor, forInNode->location);
            for (Luau::AstLocal* variable : forInNode->vars)
                binding(variable, variable->isConst);
            for (Luau::AstExpr* value : forInNode->values)
                expr(value);
            block(forInNode->body);
        }
        else if (auto assignNode = node->as<Luau::AstStatAssign>())
        {
            emit(EventTag::Assignment, assignNode->location);
            for (Luau::AstExpr* target : assignNode->vars)
                expr(target);
            for (Luau::AstExpr* value : assignNode->values)
                expr(value);
        }
        else if (auto compoundNode = node->as<Luau::AstStatCompoundAssign>())
        {
            emit(EventTag::CompoundAssignment, compoundNode->location, 0, binaryValue(compoundNode->op));
            expr(compoundNode->var);
            expr(compoundNode->value);
        }
        else if (auto functionNode = node->as<Luau::AstStatFunction>())
        {
            emit(EventTag::FunctionStatement, functionNode->location);
            functionName(functionNode->name);
            function(functionNode->func, offset(functionNode->location.begin));
        }
        else if (auto localFunctionNode = node->as<Luau::AstStatLocalFunction>())
        {
            emit(EventTag::LocalFunction, localFunctionNode->location, localFunctionNode->isConst ? kFlagConst : 0);
            binding(localFunctionNode->name, localFunctionNode->isConst);
            function(localFunctionNode->func, offset(localFunctionNode->location.begin));
        }
        else if (auto aliasNode = node->as<Luau::AstStatTypeAlias>())
        {
            emit(EventTag::TypeAlias, aliasNode->location, aliasNode->exported ? kFlagExported : 0);
            emit(EventTag::Name, aliasNode->nameLocation);
            for (Luau::AstGenericType* genericNode : aliasNode->generics)
                generic(genericNode, false);
            for (Luau::AstGenericTypePack* genericNode : aliasNode->genericPacks)
                generic(genericNode);
            typeNode(aliasNode->type);
        }
        else if (auto typeFunctionNode = node->as<Luau::AstStatTypeFunction>())
        {
            emit(EventTag::TypeFunctionStatement, typeFunctionNode->location, typeFunctionNode->exported ? kFlagExported : 0);
            emit(EventTag::Name, typeFunctionNode->nameLocation);
            function(typeFunctionNode->body, offset(typeFunctionNode->location.begin));
        }
        else if (auto globalNode = node->as<Luau::AstStatDeclareGlobal>())
        {
            emit(EventTag::DeclareGlobal, globalNode->location);
            emit(EventTag::Name, globalNode->nameLocation);
            typeNode(globalNode->type);
        }
        else if (auto declareNode = node->as<Luau::AstStatDeclareFunction>())
        {
            emit(EventTag::DeclareFunction, declareNode->location);
            emit(EventTag::Name, declareNode->nameLocation);
            for (Luau::AstGenericType* genericNode : declareNode->generics)
                generic(genericNode, false);
            for (Luau::AstGenericTypePack* genericNode : declareNode->genericPacks)
                generic(genericNode);
            for (size_t index = 0; index < declareNode->params.types.size; ++index)
            {
                const auto& name = declareNode->paramNames.data[index];
                const Luau::Location location = name.second.end > name.second.begin
                                                    ? Luau::Location(name.second.begin, declareNode->params.types.data[index]->location.end)
                                                    : declareNode->params.types.data[index]->location;
                emit(EventTag::TypeParameter, location, name.second.end > name.second.begin ? kFlagNamed : 0);
                typeNode(declareNode->params.types.data[index]);
            }
            if (declareNode->params.tailType)
                pack(declareNode->params.tailType);
            pack(declareNode->retTypes);
        }
        else if (auto classNode = node->as<Luau::AstStatClass>())
        {
            uint8_t flags = static_cast<uint8_t>(classNode->exported ? kFlagExported : 0u) | static_cast<uint8_t>(classNode->open ? kFlagOpen : 0u);
            emit(EventTag::Class, classNode->location, flags);
            binding(classNode->name, false);
            if (classNode->super)
                expr(classNode->super);
            for (const auto& member : classNode->members)
            {
                Luau::visit(
                    Luau::overloaded{
                        [&](const Luau::AstClassProperty& property)
                        {
                            emit(EventTag::ClassMember, property.nameLocation, 0);
                            emit(EventTag::Name, property.nameLocation);
                            if (property.ty)
                                typeNode(property.ty);
                        },
                        [&](const Luau::AstClassMethod& method)
                        {
                            emit(EventTag::ClassMember, method.nameLocation, kFlagMethod);
                            emit(EventTag::Name, method.nameLocation, kFlagMethod);
                            function(method.function, offset(method.function->location.begin));
                        }
                    },
                    member
                );
            }
        }
        else if (auto errorNode = node->as<Luau::AstStatError>())
        {
            emit(EventTag::Block, errorNode->location);
            for (Luau::AstExpr* expression : errorNode->expressions)
                expr(expression);
            for (Luau::AstStat* stat : errorNode->statements)
                statement(stat);
        }
    }

    void expr(Luau::AstExpr* node)
    {
        if (auto value = node->as<Luau::AstExprGroup>())
        {
            emit(EventTag::Group, value->location);
            expr(value->expr);
        }
        else if (node->is<Luau::AstExprConstantNil>())
        {
            emit(EventTag::Nil, node->location);
        }
        else if (auto value = node->as<Luau::AstExprConstantBool>())
        {
            emit(EventTag::Boolean, value->location, value->value ? 1 : 0);
        }
        else if (node->is<Luau::AstExprConstantNumber>() || node->is<Luau::AstExprConstantInteger>())
        {
            emit(EventTag::Number, node->location);
        }
        else if (node->is<Luau::AstExprConstantString>())
        {
            emit(EventTag::String, node->location);
        }
        else if (node->is<Luau::AstExprLocal>() || node->is<Luau::AstExprGlobal>())
        {
            emit(EventTag::Name, node->location);
        }
        else if (node->is<Luau::AstExprVarargs>())
        {
            emit(EventTag::Vararg, node->location);
        }
        else if (auto value = node->as<Luau::AstExprCall>())
        {
            const bool method = value->self && value->func->is<Luau::AstExprIndexName>();
            emit(EventTag::Call, value->location, method ? kFlagSelf : 0);
            if (method)
            {
                auto index = value->func->as<Luau::AstExprIndexName>();
                expr(index->expr);
                emit(EventTag::Name, index->indexLocation, kFlagMethod);
            }
            else
                expr(value->func);
            for (const Luau::AstTypeOrPack& argument : value->typeArguments)
                typeOrPack(argument);
            for (Luau::AstExpr* argument : value->args)
                expr(argument);
        }
        else if (auto value = node->as<Luau::AstExprIndexName>())
        {
            emit(EventTag::Field, value->location);
            expr(value->expr);
            emit(EventTag::Name, value->indexLocation);
        }
        else if (auto value = node->as<Luau::AstExprIndexExpr>())
        {
            emit(EventTag::Index, value->location);
            expr(value->expr);
            expr(value->index);
        }
        else if (auto value = node->as<Luau::AstExprFunction>())
        {
            function(value);
        }
        else if (auto value = node->as<Luau::AstExprTable>())
        {
            emit(EventTag::Table, value->location);
            for (const auto& item : value->items)
            {
                const Luau::Location fieldLocation =
                    item.key ? Luau::Location(item.key->location.begin, item.value->location.end) : item.value->location;
                emit(EventTag::TableField, fieldLocation);
                if (item.kind == Luau::AstExprTable::Item::Kind::Record)
                    emit(EventTag::TableKeyName, item.key->location);
                else if (item.kind == Luau::AstExprTable::Item::Kind::General)
                {
                    emit(EventTag::TableKeyExpression, item.key->location);
                    expr(item.key);
                }
                expr(item.value);
            }
        }
        else if (auto value = node->as<Luau::AstExprUnary>())
        {
            emit(EventTag::Unary, value->location, 0, unaryValue(value->op));
            expr(value->expr);
        }
        else if (auto value = node->as<Luau::AstExprBinary>())
        {
            emit(EventTag::Binary, value->location, 0, binaryValue(value->op));
            expr(value->left);
            expr(value->right);
        }
        else if (auto value = node->as<Luau::AstExprTypeAssertion>())
        {
            emit(EventTag::TypeAssertion, value->location);
            expr(value->expr);
            typeNode(value->annotation);
        }
        else if (auto value = node->as<Luau::AstExprIfElse>())
        {
            emit(EventTag::IfElse, value->location);
            expr(value->condition);
            expr(value->trueExpr);
            expr(value->falseExpr);
        }
        else if (auto value = node->as<Luau::AstExprInterpString>())
        {
            emit(EventTag::Interpolated, value->location);
            for (Luau::AstExpr* expression : value->expressions)
                expr(expression);
        }
        else if (auto value = node->as<Luau::AstExprInstantiate>())
        {
            emit(EventTag::Instantiate, value->location);
            expr(value->expr);
            for (const Luau::AstTypeOrPack& argument : value->typeArguments)
                typeOrPack(argument);
        }
        else if (auto value = node->as<Luau::AstExprError>())
        {
            emit(EventTag::Group, value->location);
            for (Luau::AstExpr* expression : value->expressions)
                expr(expression);
        }
    }

    void typeOrPack(const Luau::AstTypeOrPack& value)
    {
        if (value.type)
            typeNode(value.type);
        else if (value.typePack)
            pack(value.typePack);
    }

    void typeTable(Luau::AstTypeTable* node)
    {
        emit(EventTag::TypeTable, node->location);
        for (const Luau::AstTableProp& property : node->props)
        {
            emit(EventTag::TypeField, property.location, 0);
            emit(EventTag::Name, property.location);
            typeNode(property.type);
        }
        if (node->indexer)
        {
            emit(EventTag::TypeIndexer, node->indexer->location);
            typeNode(node->indexer->indexType);
            typeNode(node->indexer->resultType);
        }
    }

    void typeFunction(Luau::AstTypeFunction* node)
    {
        emit(EventTag::TypeFunction, node->location);
        attributes(node->attributes);
        for (Luau::AstGenericType* genericNode : node->generics)
            generic(genericNode, false);
        for (Luau::AstGenericTypePack* genericNode : node->genericPacks)
            generic(genericNode);
        for (size_t index = 0; index < node->argTypes.types.size; ++index)
        {
            const auto& name = node->argNames.data[index];
            emit(EventTag::TypeParameter, node->argTypes.types.data[index]->location, name.has_value() ? kFlagNamed : 0);
            typeNode(node->argTypes.types.data[index]);
        }
        if (node->argTypes.tailType)
            pack(node->argTypes.tailType);
        pack(node->returnTypes);
    }

    void typeNode(Luau::AstType* node)
    {
        if (auto value = node->as<Luau::AstTypeReference>())
        {
            if (value->name == "nil")
            {
                emit(EventTag::TypeNil, value->location);
                return;
            }
            emit(EventTag::TypeName, value->location);
            if (value->prefix && value->prefixLocation)
            {
                const Luau::Location& prefixLocation = *value->prefixLocation;
                emitSpan(EventTag::Name, offset(prefixLocation.begin), offset(prefixLocation.end));
            }
            emit(EventTag::Name, value->nameLocation);
            for (const Luau::AstTypeOrPack& argument : value->parameters)
                typeOrPack(argument);
        }
        else if (auto value = node->as<Luau::AstTypeTable>())
        {
            typeTable(value);
        }
        else if (auto value = node->as<Luau::AstTypeFunction>())
        {
            typeFunction(value);
        }
        else if (auto value = node->as<Luau::AstTypeTypeof>())
        {
            emit(EventTag::Typeof, value->location);
            expr(value->expr);
        }
        else if (node->is<Luau::AstTypeOptional>())
        {
            emit(EventTag::TypeOptional, node->location);
        }
        else if (auto value = node->as<Luau::AstTypeUnion>())
        {
            emit(EventTag::TypeUnion, value->location);
            for (size_t index = 0; index < value->types.size; ++index)
            {
                if (!value->types.data[index]->is<Luau::AstTypeOptional>())
                {
                    if (index + 1 < value->types.size && value->types.data[index + 1]->is<Luau::AstTypeOptional>())
                    {
                        const Luau::Location optionalLocation(value->types.data[index]->location.begin, value->types.data[index + 1]->location.end);
                        emit(EventTag::TypeOptional, optionalLocation);
                    }
                    typeNode(value->types.data[index]);
                }
            }
        }
        else if (auto value = node->as<Luau::AstTypeIntersection>())
        {
            emit(EventTag::TypeIntersection, value->location);
            for (Luau::AstType* member : value->types)
                typeNode(member);
        }
        else if (auto value = node->as<Luau::AstTypeSingletonBool>())
        {
            emit(EventTag::TypeBoolean, value->location, value->value ? 1 : 0);
        }
        else if (node->is<Luau::AstTypeSingletonString>())
        {
            emit(EventTag::TypeString, node->location);
        }
        else if (auto value = node->as<Luau::AstTypeGroup>())
        {
            emit(EventTag::TypeGroup, value->location);
            typeNode(value->type);
        }
        else if (auto value = node->as<Luau::AstTypeError>())
        {
            emit(EventTag::TypeGroup, value->location);
            for (Luau::AstType* member : value->types)
                typeNode(member);
        }
    }

    void pack(Luau::AstTypePack* node)
    {
        if (auto value = node->as<Luau::AstTypePackExplicit>())
        {
            emit(EventTag::TypePack, value->location);
            for (Luau::AstType* member : value->typeList.types)
                typeNode(member);
            if (value->typeList.tailType)
                pack(value->typeList.tailType);
        }
        else if (auto value = node->as<Luau::AstTypePackVariadic>())
        {
            emit(EventTag::TypePackVariadic, value->location);
            typeNode(value->variadicType);
        }
        else if (auto value = node->as<Luau::AstTypePackGeneric>())
        {
            emit(EventTag::TypePackGeneric, value->location);
        }
    }
};

std::vector<size_t> lineStartsFor(const std::vector<char>& source)
{
    std::vector<size_t> lineStarts = {0};
    for (size_t index = 0; index < source.size(); ++index)
    {
        if (source[index] == '\n')
            lineStarts.push_back(index + 1);
    }
    return lineStarts;
}

ParseOutput parse(const std::vector<char>& source, Request request)
{
    Luau::Allocator allocator;
    Luau::AstNameTable names(allocator);
    Luau::ParseOptions options;
    // Match Vermis's supported declaration and syntax extensions.
    options.allowDeclarationSyntax = true;
    FFlag::LuauExportValueSyntax.value = true;
    FFlag::LuauIntegerType2.value = true;
    FFlag::DebugLuauIfLocalSyntax.value = true;
    FFlag::DebugLuauUserDefinedClasses.value = true;
    const auto lineStarts = lineStartsFor(source);
    AstSerializer serializer(lineStarts, source.size());

    if (request == Request::Chunk)
    {
        Luau::ParseResult result = Luau::Parser::parse(source.data(), source.size(), names, allocator, options);
        ParseOutput output;
        for (const Luau::ParseError& error : result.errors)
        {
            const Luau::Location& location = error.getLocation();
            output.errors.push_back({
                offsetFor(location.begin, lineStarts, source.size()),
                offsetFor(location.end, lineStarts, source.size()),
            });
        }
        if (output.errors.empty() && result.root)
            return serializer.chunk(result.root);
        return output;
    }
    if (request == Request::Expression)
    {
        Luau::ParseNodeResult<Luau::AstExpr> result = Luau::Parser::parseExpr(source.data(), source.size(), names, allocator, options);
        ParseOutput output;
        for (const Luau::ParseError& error : result.errors)
        {
            const Luau::Location& location = error.getLocation();
            output.errors.push_back({
                offsetFor(location.begin, lineStarts, source.size()),
                offsetFor(location.end, lineStarts, source.size()),
            });
        }
        if (output.errors.empty() && result.root)
            return serializer.expression(result.root);
        return output;
    }

    Luau::ParseNodeResult<Luau::AstType> result = Luau::Parser::parseType(source.data(), source.size(), names, allocator, options);
    ParseOutput output;
    for (const Luau::ParseError& error : result.errors)
    {
        const Luau::Location& location = error.getLocation();
        output.errors.push_back({
            offsetFor(location.begin, lineStarts, source.size()),
            offsetFor(location.end, lineStarts, source.size()),
        });
    }
    if (output.errors.empty() && result.root)
        return serializer.type(result.root);
    return output;
}

bool writeParseResult(const ParseOutput& output)
{
    if (output.events.size() > std::numeric_limits<uint32_t>::max() || output.errors.size() > std::numeric_limits<uint32_t>::max())
        return false;

    const uint8_t accepted = output.errors.empty() ? 1 : 0;
    writeBytes(&accepted, sizeof(accepted));
    writeU32(static_cast<uint32_t>(output.errors.size()));
    for (const ParseErrorSpan& error : output.errors)
    {
        writeU64(error.start);
        writeU64(error.end);
    }
    writeU32(static_cast<uint32_t>(output.events.size()));
    for (const Event& event : output.events)
    {
        const uint8_t tag = static_cast<uint8_t>(event.tag);
        writeBytes(&tag, sizeof(tag));
        writeBytes(&event.flags, sizeof(event.flags));
        writeU16(0);
        writeU64(event.start);
        writeU64(event.end);
        writeU32(event.value);
    }
    return std::fflush(stdout) == 0;
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
        uint8_t requestValue = 0;
        if (!readBytes(&requestValue, sizeof(requestValue)))
        {
            return std::feof(stdin) ? 0 : 1;
        }
        if (requestValue > static_cast<uint8_t>(Request::Type))
        {
            return 1;
        }

        uint64_t size = 0;
        if (!readU64(size))
        {
            return 1;
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

        const Request request = static_cast<Request>(requestValue);
        if (request == Request::Lex)
        {
            if (!writeResult(lex(source)))
                return 1;
        }
        else if (!writeParseResult(parse(source, request)))
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
