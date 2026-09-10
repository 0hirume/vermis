def --wrapped main [
    target: string # Fuzz target to execute.
    ...arguments: string # Arguments forwarded to libFuzzer.
]: nothing -> nothing {
    let discovery = (
        $env
        | get 'ProgramFiles(x86)'
        | path join 'Microsoft Visual Studio' Installer vswhere.exe
    )

    let installation = (
        ^$discovery -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        | complete
    )

    if $installation.exit_code != 0 {
        print --stderr $installation.stderr
        exit $installation.exit_code
    }

    let directory = $installation.stdout | str trim

    if ($directory | is-empty) {
        print --stderr 'Visual Studio C++ tools are required for fuzzing.'
        exit 1
    }

    let developer = $directory | path join Common7 Tools VsDevCmd.bat

    let suffix = if ($arguments | is-empty) { '' } else { $" -- ($arguments | str join ' ')" }

    let commands = [
        $'call "($developer)" -arch=x64'
        'if errorlevel 1 exit /b %errorlevel%'
        $'cargo fuzz run ($target)($suffix)'
        'exit /b %errorlevel%'
        ''
    ] | str join (char crlf)

    try {
        $commands | ^$env.ComSpec /d
    } catch {|error| exit $error.exit_code }

    exit 0
}
