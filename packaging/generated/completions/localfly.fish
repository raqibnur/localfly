# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_localfly_global_optspecs
    string join \n h/help V/version
end

function __fish_localfly_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_localfly_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_localfly_using_subcommand
    set -l cmd (__fish_localfly_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c localfly -n "__fish_localfly_needs_command" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_needs_command" -s V -l version -d 'Print version'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "add" -d 'Save an app by its local port'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "list" -d 'List saved apps with live status dots'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "rm" -d 'Remove a saved app (by id prefix, name, or port)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "edit" -d 'Edit a saved app'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "on" -d 'Expose a saved app in the daemon (follows status; Ctrl-C detaches, tunnel keeps running)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "off" -d 'Stop a saved app\'s tunnel (or --all)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "expose" -d 'One-shot tunnel without saving (foreground; Ctrl-C stops it)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "up" -d 'Start every app marked start-on-launch in the daemon (foreground follow; SIGTERM stops all)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "status" -d 'Show live tunnels and their traffic'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "probe" -d 'Check whether something listens on a port'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "scan" -d 'Scan localhost for common dev-server ports'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "relay-status" -d 'Measure relay control-port latency'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "config" -d 'Persist defaults (mirrors macOS Settings: relay, auto-reconnect, local host)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "update-check" -d 'Check GitHub releases for a newer version (read-only, like UpdateChecker)'
complete -c localfly -n "__fish_localfly_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c localfly -n "__fish_localfly_using_subcommand add" -l name -r
complete -c localfly -n "__fish_localfly_using_subcommand add" -l port -r
complete -c localfly -n "__fish_localfly_using_subcommand add" -l public-port -r
complete -c localfly -n "__fish_localfly_using_subcommand add" -l public-address -r
complete -c localfly -n "__fish_localfly_using_subcommand add" -l start-on-launch
complete -c localfly -n "__fish_localfly_using_subcommand add" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand list" -l json
complete -c localfly -n "__fish_localfly_using_subcommand list" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand rm" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l name -r
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l port -r
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l public-port -r
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l public-address -r
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l start-on-launch -r -f -a "true\t''
false\t''"
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l no-public-port
complete -c localfly -n "__fish_localfly_using_subcommand edit" -l no-public-address
complete -c localfly -n "__fish_localfly_using_subcommand edit" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand on" -l relay -r
complete -c localfly -n "__fish_localfly_using_subcommand on" -l secret -r
complete -c localfly -n "__fish_localfly_using_subcommand on" -l local-host -r
complete -c localfly -n "__fish_localfly_using_subcommand on" -l no-reconnect
complete -c localfly -n "__fish_localfly_using_subcommand on" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand off" -l all
complete -c localfly -n "__fish_localfly_using_subcommand off" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand expose" -l remote-port -r
complete -c localfly -n "__fish_localfly_using_subcommand expose" -l relay -r
complete -c localfly -n "__fish_localfly_using_subcommand expose" -l secret -r
complete -c localfly -n "__fish_localfly_using_subcommand expose" -l local-host -r
complete -c localfly -n "__fish_localfly_using_subcommand expose" -l no-reconnect
complete -c localfly -n "__fish_localfly_using_subcommand expose" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand up" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand status" -l json
complete -c localfly -n "__fish_localfly_using_subcommand status" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand probe" -l host -r
complete -c localfly -n "__fish_localfly_using_subcommand probe" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand scan" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand relay-status" -l relay -r
complete -c localfly -n "__fish_localfly_using_subcommand relay-status" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand config" -l relay -r
complete -c localfly -n "__fish_localfly_using_subcommand config" -l auto-reconnect -r -f -a "true\t''
false\t''"
complete -c localfly -n "__fish_localfly_using_subcommand config" -l local-host -r
complete -c localfly -n "__fish_localfly_using_subcommand config" -l secret -r
complete -c localfly -n "__fish_localfly_using_subcommand config" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand update-check" -s h -l help -d 'Print help'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "add" -d 'Save an app by its local port'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "list" -d 'List saved apps with live status dots'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "rm" -d 'Remove a saved app (by id prefix, name, or port)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "edit" -d 'Edit a saved app'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "on" -d 'Expose a saved app in the daemon (follows status; Ctrl-C detaches, tunnel keeps running)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "off" -d 'Stop a saved app\'s tunnel (or --all)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "expose" -d 'One-shot tunnel without saving (foreground; Ctrl-C stops it)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "up" -d 'Start every app marked start-on-launch in the daemon (foreground follow; SIGTERM stops all)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "status" -d 'Show live tunnels and their traffic'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "probe" -d 'Check whether something listens on a port'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "scan" -d 'Scan localhost for common dev-server ports'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "relay-status" -d 'Measure relay control-port latency'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "config" -d 'Persist defaults (mirrors macOS Settings: relay, auto-reconnect, local host)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "update-check" -d 'Check GitHub releases for a newer version (read-only, like UpdateChecker)'
complete -c localfly -n "__fish_localfly_using_subcommand help; and not __fish_seen_subcommand_from add list rm edit on off expose up status probe scan relay-status config update-check help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
