# lopi completion for PowerShell (5.1 and 7+).
# Install: add this line to your profile (notepad $PROFILE)
#   lopi completion powershell | Out-String | Invoke-Expression
Register-ArgumentCompleter -Native -CommandName lopi, lopi.exe -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $words = @($commandAst.CommandElements | ForEach-Object { $_.ToString() })
    # Index of the word being completed; 0 is the command itself.
    $index = $words.Count
    if ($wordToComplete -ne '') { $index -= 1 }

    $candidates = @()
    if ($index -eq 1) {
        $candidates = @(lopi __complete 2>$null) + @(@SUBCOMMANDS@)
    } elseif ($index -eq 2 -and @(@PROFILE_SUBCOMMANDS@) -contains $words[1]) {
        $candidates = @(lopi __complete 2>$null)
    } elseif ($index -eq 2 -and $words[1] -eq 'completion') {
        $candidates = @(@SHELLS@)
    }
    $candidates |
        Where-Object { $_ -like "$wordToComplete*" } |
        ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
        }
}
