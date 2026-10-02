#compdef lopi
# lopi completion for zsh.
# Install: add this line to ~/.zshrc, after `compinit`
#   eval "$(lopi completion zsh)"
_lopi() {
  local -a profiles subcommands
  profiles=(${(f)"$(lopi __complete 2>/dev/null)"})
  subcommands=(@SUBCOMMANDS@)
  if (( CURRENT == 2 )); then
    compadd -a profiles subcommands
  elif (( CURRENT == 3 )) && [[ ${words[2]} == (@PROFILE_SUBCOMMANDS@) ]]; then
    compadd -a profiles
  elif (( CURRENT == 3 )) && [[ ${words[2]} == completion ]]; then
    compadd @SHELLS@
  else
    _default
  fi
}
compdef _lopi lopi
