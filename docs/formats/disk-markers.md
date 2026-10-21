# `xenobot.1` and `xenobot.2` disk markers

**Confirmed** (T-024), from the install scripts. Each is 4 bytes: its floppy disk's
number as a digit, a space and `CR LF`. `install.bat` waits until
`<source>\Xenobot.1` exists before it copies disk 1, and afterwards reports an error
(a disk-space check) if `Xenobot.1` did not reach the destination. `part2.bat` does
the same for disk 2 with `Xenobot.2`. They end up in the install because the scripts
copy `*.*`. No game binary names them, so they have no parser and are not in
`xeno_formats::files::EXPECTED_FILES`.
