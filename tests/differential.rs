#[path = "support/oracle.rs"]
mod oracle;

use std::fs;
use std::path::{Path, PathBuf};

fn oracle() -> oracle::Oracle {
    let path = std::env::var_os("VERMIS_ORACLE")
        .map(PathBuf::from)
        .expect("VERMIS_ORACLE must point to the Luau oracle executable");

    oracle::Oracle::spawn(&path).expect("failed to start Luau oracle")
}

fn check(oracle: &mut oracle::Oracle, name: &str, source: &[u8]) {
    if let Err(error) = oracle::compare(oracle, source) {
        panic!("{name}: {error}");
    }
}

fn walk(path: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let path = entry.path();

        if path.is_dir() {
            walk(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "lua" || extension == "luau")
        {
            files.push(path);
        }
    }
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn samples() {
    let cases = [
        b"".as_slice(),
        b" \t\r\n\x0b\x0c".as_slice(),
        b"local x = 1 + 2 * 3".as_slice(),
        b"type T = {read x: number, write y: string}".as_slice(),
        b"continue() continue".as_slice(),
        b"-- line\n--[[ block ]]".as_slice(),
        b"[=[raw]=] 'quoted' \"double\"".as_slice(),
        b"`plain` `{x} and {y}`".as_slice(),
        b"@native@[deprecated]".as_slice(),
        b"0xABC 0b101 1_000 .5 1e+2".as_slice(),
        b"== <= >= ~= .. ... -> :: // += -= *= /= //= %= ^= ..=".as_slice(),
        b"\xff\xfe\xe2!\xe2\x98\x83".as_slice(),
        b"local x = 1\0tail".as_slice(),
        b"[=x --[[x 'x `x `{{x}`".as_slice(),
    ];
    let mut oracle = oracle();

    for (index, source) in cases.iter().enumerate() {
        check(&mut oracle, &format!("sample {index}"), source);
    }
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn upstream_lexer_cases() {
    let cases = [
        b"[[".as_slice(),
        b"--[[  ".as_slice(),
        b"--  ".as_slice(),
        b"--[[ function \n]] end".as_slice(),
        b"'\\3729472897292378'".as_slice(),
        b"--[===[\n\n\n\n]===]".as_slice(),
        b"foo --[[ comment ]] bar : nil end".as_slice(),
        b"`foo {\"bar\"}`".as_slice(),
        b"`foo {\"bar\"} {\"baz\"} end`".as_slice(),
        b"`foo{{bad}}bar`".as_slice(),
        b"`{{oops}`, 1".as_slice(),
        b"{\n        `hello {\"world\"}\n    } -- this might be incorrectly parsed as a string"
            .as_slice(),
        b"`\\u{1F41B}`".as_slice(),
        b"'test'".as_slice(),
        b"\"test\"".as_slice(),
        b"[[ test ]]".as_slice(),
        b"[[ test\n    ]]".as_slice(),
        b"[[\n    test\n    ]]".as_slice(),
        b"[[\n    test ]]".as_slice(),
        b"[=[[%s]]=]".as_slice(),
        b"[==[ test ]==]".as_slice(),
        b"[==[ test\n    ]==]".as_slice(),
        b"[==[\n    test\n    ]==]".as_slice(),
        b"[==[\n\n    test ]==]".as_slice(),
        b"--[[ test ]]".as_slice(),
        b"--[=[ \xce\xbc\xce\xad\xce\xbb\xce\xbb\xce\xbf\xce\xbd ]=]".as_slice(),
        b"--[==[ test ]==]".as_slice(),
    ];
    let mut oracle = oracle();

    for (index, source) in cases.iter().enumerate() {
        check(&mut oracle, &format!("upstream case {index}"), source);
    }
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn nul_tails() {
    let prefixes = [
        b"".as_slice(),
        b"'x".as_slice(),
        b"[[x".as_slice(),
        b"`x".as_slice(),
        b"--x".as_slice(),
        b"--[[x".as_slice(),
        b"`{x".as_slice(),
        b"`{{x".as_slice(),
        b"{`x".as_slice(),
    ];
    let suffixes = [
        b"".as_slice(),
        b"tail".as_slice(),
        b" --[[tail]]".as_slice(),
        b"`tail".as_slice(),
        b"\xfftail".as_slice(),
    ];
    let mut oracle = oracle();

    for (prefix_index, prefix) in prefixes.iter().enumerate() {
        for (suffix_index, suffix) in suffixes.iter().enumerate() {
            let mut source = Vec::with_capacity(prefix.len() + suffix.len() + 1);
            source.extend_from_slice(prefix);
            source.push(0);
            source.extend_from_slice(suffix);
            check(
                &mut oracle,
                &format!("NUL case {prefix_index}:{suffix_index}"),
                &source,
            );
        }
    }
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn longer_inputs() {
    let mut oracle = oracle();

    for byte in u8::MIN..=u8::MAX {
        let source = vec![byte; 257];
        check(&mut oracle, &format!("repeated byte {byte}"), &source);
    }

    let ascending = (u8::MIN..=u8::MAX).collect::<Vec<_>>();
    check(&mut oracle, "ascending bytes", &ascending);

    let descending = (u8::MIN..=u8::MAX).rev().collect::<Vec<_>>();
    check(&mut oracle, "descending bytes", &descending);
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn byte_pairs() {
    let mut oracle = oracle();

    for first in u8::MIN..=u8::MAX {
        for second in u8::MIN..=u8::MAX {
            let source = [first, second];
            check(&mut oracle, "byte pair", &source);
        }
    }
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn corpus() {
    let mut files = Vec::new();
    walk(Path::new("vendor/luau/tests/conformance"), &mut files);
    if Path::new("fuzz/corpus/lexer").exists() {
        walk(Path::new("fuzz/corpus/lexer"), &mut files);
    }
    files.sort();

    let mut oracle = oracle();

    for path in files {
        let source = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        check(&mut oracle, &path.display().to_string(), &source);
    }
}
