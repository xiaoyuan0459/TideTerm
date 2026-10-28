# TideTerm's prompt callback observes Bash history, never a raw keystroke stream.
__tideterm_prompt() {
    local __tt_status=$? LC_ALL=C
    local __tt_entry __tt_id __tt_command __tt_encoded='' __tt_byte __tt_i
    __tt_entry=$(HISTTIMEFORMAT= builtin history 1)
    __tt_entry=${__tt_entry#"${__tt_entry%%[![:space:]]*}"}
    __tt_id=${__tt_entry%%[[:space:]]*}
    __tt_command=${__tt_entry#"$__tt_id"}
    __tt_command=${__tt_command# }
    [[ $__tt_id == *\* ]] || __tt_command=${__tt_command# }
    if [[ ${__tideterm_history_ready-} != 1 ]]; then
        __tideterm_history_ready=1
        __tideterm_history_id=$__tt_id
        printf '\033]777;tideterm-history\007'
        printf '\033]133;A\007'
        return "$__tt_status"
    fi
    if [[ $__tt_id != "${__tideterm_history_id-}" ]]; then
        __tideterm_history_id=$__tt_id
        if [[ $__tt_id =~ ^[0-9]+\*?$ ]] && (( ${#__tt_command} > 0 && ${#__tt_command} <= 4096 )); then
            for (( __tt_i=0; __tt_i<${#__tt_command}; __tt_i++ )); do
                printf -v __tt_byte '%%%02X' "'${__tt_command:__tt_i:1}"
                __tt_encoded+=$__tt_byte
            done
            printf '\033]777;tideterm-command;%s\007' "$__tt_encoded"
        fi
    fi
    printf '\033]133;A\007'
    return "$__tt_status"
}
if (( BASH_VERSINFO[0] > 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 1) )); then
    PROMPT_COMMAND+=(__tideterm_prompt)
else
    PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND; }__tideterm_prompt"
fi
