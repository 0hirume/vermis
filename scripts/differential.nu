let mode = ($env.VERMIS_DIFFERENTIAL_MODE? | default test)

if $mode != test and $mode != fuzz {
    error make {
        msg: 'The differential script mode must be test or fuzz.'
        labels: [
            {text: 'Invalid mode', span: (metadata $mode).span}
        ]
    }
}

let verification_command = if $mode == fuzz {
    'cargo fuzz run differential'
} else {
    'cargo test --test differential --locked -- --ignored --nocapture'
}

let root_path = (pwd | path expand)
let vswhere_path = ($env.'ProgramFiles(x86)' | path join 'Microsoft Visual Studio' Installer vswhere.exe)
let luau_path = ($root_path | path join vendor luau)
let oracle_source_path = ($root_path | path join tests oracle)
let oracle_build_path = ($root_path | path join target oracle)
let oracle_name = if $nu.os-info.name == windows {
    'vermis-luau-oracle.exe'
} else {
    'vermis-luau-oracle'
}
let oracle_path = ($oracle_build_path | path join bin $oracle_name)

if not ($vswhere_path | path exists) {
    error make {
        msg: 'Visual Studio Installer discovery tool was not found.'
        labels: [
            {text: 'Missing executable', span: (metadata $vswhere_path).span}
        ]
    }
}

if not ($luau_path | path join CMakeLists.txt | path exists) {
    error make {
        msg: 'The pinned Luau submodule is not available.'
        labels: [
            {text: 'Missing Luau checkout', span: (metadata $luau_path).span}
        ]
    }
}

let discovery_result = (^$vswhere_path -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | complete)

if $discovery_result.exit_code != 0 {
    print --stderr $discovery_result.stderr
    exit $discovery_result.exit_code
}

let installation_path = ($discovery_result.stdout | str trim)

if ($installation_path | is-empty) {
    error make {
        msg: 'No Visual Studio installation with MSVC C++ tools was found.'
        labels: [
            {text: 'Empty discovery result', span: (metadata $installation_path).span}
        ]
    }
}

let developer_script_path = ($installation_path | path join Common7 Tools VsDevCmd.bat)

if not ($developer_script_path | path exists) {
    error make {
        msg: 'The selected Visual Studio installation has no VsDevCmd.bat.'
        labels: [
            {text: 'Missing script', span: (metadata $developer_script_path).span}
        ]
    }
}

let batch_commands = ([
    'call "%VERMIS_VSDEVCMD%" -arch=x64'
    'if errorlevel 1 exit /b %errorlevel%'
    'cmake --fresh -G "NMake Makefiles" -S "%VERMIS_ORACLE_SOURCE%" -B "%VERMIS_ORACLE_BUILD%" -DLUAU_SOURCE_DIR="%VERMIS_LUAU%" -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_EXPORT_COMPILE_COMMANDS=ON'
    'if errorlevel 1 exit /b %errorlevel%'
    'clang-format --style=file:vendor/luau/.clang-format --dry-run --Werror "%VERMIS_ORACLE_SOURCE%\\oracle.cpp"'
    'if errorlevel 1 exit /b %errorlevel%'
    'cmake --build "%VERMIS_ORACLE_BUILD%" --target vermis-luau-oracle'
    'if errorlevel 1 exit /b %errorlevel%'
    'clangd --check="%VERMIS_ORACLE_SOURCE%\\oracle.cpp" --compile-commands-dir="%VERMIS_ORACLE_BUILD%" --enable-config --log=error'
    'if errorlevel 1 exit /b %errorlevel%'
    'clang-tidy "%VERMIS_ORACLE_SOURCE%\\oracle.cpp" -p="%VERMIS_ORACLE_BUILD%" -checks=-*,bugprone-*,performance-*,readability-magic-numbers -header-filter=".*[\\\\/]tests[\\\\/]oracle[\\\\/].*" --warnings-as-errors=*'
    'if errorlevel 1 exit /b %errorlevel%'
    'set "VERMIS_ORACLE=%VERMIS_ORACLE_PATH%"'
    $verification_command
    'exit /b %errorlevel%'
    ''
] | str join (char crlf))

let environment = {
    VERMIS_VSDEVCMD: $developer_script_path
    VERMIS_LUAU: $luau_path
    VERMIS_ORACLE: $oracle_path
    VERMIS_ORACLE_BUILD: $oracle_build_path
    VERMIS_ORACLE_PATH: $oracle_path
    VERMIS_ORACLE_SOURCE: $oracle_source_path
}

with-env $environment {
    if $mode == fuzz {
        try {
            $batch_commands | ^$env.ComSpec /d
        } catch {|error|
            exit $error.exit_code
        }

        exit 0
    }

    let process_result = ($batch_commands | ^$env.ComSpec /d | complete)

    print $process_result.stdout
    print --stderr $process_result.stderr

    exit $process_result.exit_code
}
