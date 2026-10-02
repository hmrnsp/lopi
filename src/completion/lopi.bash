# lopi completion for bash.
# Install: add this line to ~/.bashrc
#   eval "$(lopi completion bash)"
_lopi() {
    local cur=${COMP_WORDS[COMP_CWORD]}
    local sub=${COMP_WORDS[1]}
    if [[ $COMP_CWORD -eq 1 ]]; then
        COMPREPLY=($(compgen -W "$(lopi __complete 2>/dev/null) @SUBCOMMANDS@" -- "$cur"))
    elif [[ $COMP_CWORD -eq 2 && " @PROFILE_SUBCOMMANDS@ " == *" $sub "* ]]; then
        COMPREPLY=($(compgen -W "$(lopi __complete 2>/dev/null)" -- "$cur"))
    elif [[ $COMP_CWORD -eq 2 && $sub == completion ]]; then
        COMPREPLY=($(compgen -W "@SHELLS@" -- "$cur"))
    else
        COMPREPLY=()
    fi
}
complete -o bashdefault -o default -F _lopi lopi lopi.exe
