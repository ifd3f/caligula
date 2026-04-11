def print_logs(machine):
    _, output = machine.execute(
        'for x in $(find /tmp/caligula-* -type f); do echo "$x"; cat "$x"; echo; done',
        check_output=True,
    )
    print(output)
