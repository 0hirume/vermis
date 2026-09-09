let vswhere_path = ($env.'ProgramFiles(x86)' | path join 'Microsoft Visual Studio' Installer vswhere.exe)

if not ($vswhere_path | path exists) {
    error make {
        msg: 'Visual Studio Installer discovery tool was not found.'
        labels: [
            {text: 'Missing executable', span: (metadata $vswhere_path).span}
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

with-env {VERMIS_VSDEVCMD: $developer_script_path} {
    [
        'call "%VERMIS_VSDEVCMD%" -arch=x64 && cargo fuzz run lexer'
        'exit /b %errorlevel%'
        ''
    ] | str join (char crlf) |

    exit (^$env.ComSpec /d | complete).exit_code
}
