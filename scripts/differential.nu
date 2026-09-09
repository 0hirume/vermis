def --wrapped main [
    mode: string = "test" # Verification mode to run.
    ...fuzz_args: string # Arguments to pass to libFuzzer.
]: nothing -> nothing {
    if $mode != test and $mode != fuzz {
        error make {
            msg: 'The differential script mode must be test or fuzz.'
            labels: [
                {
                    text: 'Invalid mode'
                    span: (metadata $mode).span
                }
            ]
        }
    }

    let fuzz_suffix = if ($fuzz_args | is-empty) {
        ''
    } else {
        $" -- ($fuzz_args | str join ' ')"
    }
    let verification_command = if $mode == fuzz {
        $"cargo fuzz run differential($fuzz_suffix)"
    } else {
        'cargo test --test differential --locked -- --ignored --nocapture'
    }

    let root_path = pwd | path expand
    let vswhere_path = (
        $env
        | get 'ProgramFiles(x86)'
        | path join 'Microsoft Visual Studio' Installer vswhere.exe
    )
    let luau_path = $root_path | path join vendor luau
    let oracle_source_path = $root_path | path join tests oracle
    let oracle_source_file_path = $oracle_source_path | path join oracle.cpp
    let oracle_build_path = $root_path | path join target oracle
    let oracle_name = if $nu.os-info.name == windows {
        'vermis-luau-oracle.exe'
    } else {
        'vermis-luau-oracle'
    }
    let oracle_path = $oracle_build_path | path join bin $oracle_name

    if not ($vswhere_path | path exists) {
        error make {
            msg: 'Visual Studio Installer discovery tool was not found.'
            labels: [
                {
                    text: 'Missing executable'
                    span: (metadata $vswhere_path).span
                }
            ]
        }
    }

    if not ($luau_path | path join CMakeLists.txt | path exists) {
        error make {
            msg: 'The pinned Luau submodule is not available.'
            labels: [
                {
                    text: 'Missing Luau checkout'
                    span: (metadata $luau_path).span
                }
            ]
        }
    }

    let discovery_result = (
        ^$vswhere_path -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        | complete
    )

    if $discovery_result.exit_code != 0 {
        print --stderr $discovery_result.stderr
        exit $discovery_result.exit_code
    }

    let installation_path = $discovery_result.stdout | str trim

    if ($installation_path | is-empty) {
        error make {
            msg: 'No Visual Studio installation with MSVC C++ tools was found.'
            labels: [
                {
                    text: 'Empty discovery result'
                    span: (metadata $installation_path).span
                }
            ]
        }
    }

    let developer_script_path = $installation_path | path join Common7 Tools VsDevCmd.bat

    if not ($developer_script_path | path exists) {
        error make {
            msg: 'The selected Visual Studio installation has no VsDevCmd.bat.'
            labels: [
                {
                    text: 'Missing script'
                    span: (metadata $developer_script_path).span
                }
            ]
        }
    }

    let batch_commands = ([
        $'call "($developer_script_path)" -arch=x64'
        'if errorlevel 1 exit /b %errorlevel%'
        $'cmake --fresh -G "NMake Makefiles" -S "($oracle_source_path)" -B "($oracle_build_path)" -DLUAU_SOURCE_DIR="($luau_path)" -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_EXPORT_COMPILE_COMMANDS=ON'
        'if errorlevel 1 exit /b %errorlevel%'
        $'clang-format --style=file:vendor/luau/.clang-format --dry-run --Werror "($oracle_source_file_path)"'
        'if errorlevel 1 exit /b %errorlevel%'
        $'cmake --build "($oracle_build_path)" --target vermis-luau-oracle'
        'if errorlevel 1 exit /b %errorlevel%'
        $'clangd --check="($oracle_source_file_path)" --compile-commands-dir="($oracle_build_path)" --enable-config --log=error'
        'if errorlevel 1 exit /b %errorlevel%'
        $'clang-tidy "($oracle_source_file_path)" -p="($oracle_build_path)" -checks=-*,bugprone-*,performance-*,readability-magic-numbers -header-filter=".*[\\\\/]tests[\\\\/]oracle[\\\\/].*" --warnings-as-errors=*'
        'if errorlevel 1 exit /b %errorlevel%'
        $'set "VERMIS_ORACLE=($oracle_path)"'
        $verification_command
        'exit /b %errorlevel%'
        ''
    ] | str join (char crlf))

    if $mode == fuzz {
        try {
            $batch_commands | ^$env.ComSpec /d
        } catch {|error| exit $error.exit_code }

        exit 0
    }

    let process_result = $batch_commands | ^$env.ComSpec /d | complete

    print $process_result.stdout
    print --stderr $process_result.stderr

    exit $process_result.exit_code
}
