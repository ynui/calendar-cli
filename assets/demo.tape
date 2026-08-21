# Regenerate assets/demo.gif:
#   1. Build a sandbox so no real credentials/calendar are touched:
#        SANDBOX=/tmp/cal-demo
#        mkdir -p "$SANDBOX/Library/Application Support/calendar-cli"
#        echo '{"installed":{"client_id":"demo","client_secret":"demo"}}' > "$SANDBOX/Library/Application Support/calendar-cli/credentials.json"
#        # optional: seed $SANDBOX/.../calendar-cli/events.json with fake events
#        # remove $SANDBOX/.../calendar-cli/settings.json to start from defaults
#   2. Create a ./demo-calendar wrapper next to this tape:
#        #!/bin/sh
#        HOME=<SANDBOX> exec /path/to/target/release/calendar-cli "$@"
#   3. Run: vhs assets/demo.tape

Output assets/demo.gif

Set FontSize 20
Set Width 1280
Set Height 780
Set Padding 24
Set BorderRadius 10
Set WindowBar Colorful

Type@0.06 "./demo-calendar"
Enter
Sleep 2500ms

Right@500ms Down@400ms Right@500ms
Sleep 1500ms

Left@500ms Up@400ms
Sleep 1200ms

Type "w"
Sleep 1200ms
Right@400ms Right@400ms Right@600ms
Sleep 1800ms
Type "w"
Sleep 1000ms

Type "s"
Sleep 1500ms

Down@500ms
Sleep 300ms
Down@500ms
Sleep 300ms
Down@600ms
Sleep 600ms
Enter
Sleep 1400ms
Enter
Sleep 1400ms
Enter
Sleep 1400ms
Enter
Sleep 1400ms
Enter
Sleep 1600ms

Down@700ms
Sleep 500ms
Enter
Sleep 1400ms
Enter
Sleep 1400ms
Enter
Sleep 1400ms
Enter
Sleep 2400ms

Escape
Sleep 800ms

Type "t"
Sleep 800ms

Type "n"
Sleep 700ms
Type@0.04 "Pizza night"
Tab Tab
Delete Delete Delete Delete Delete
Type@0.04 "18:00"
Tab
Delete Delete Delete Delete Delete
Type@0.04 "19:00"
Sleep 400ms
Enter
Sleep 2500ms
