# NBT fixtures

`servers-with-unknown-tags.dat`: an uncompressed `servers.dat` written by an
independent minimal NBT encoder. Root compound with a `servers` list of two
entries and an unknown root tag `launcherNote` (Int 42). The first entry has
`acceptTextures` (Byte 1) and an unknown compound `forgeData`
(`fmlNetworkVersion` Int 3, `truncated` Byte 0); the second has `hidden`
(Byte 0) and `icon`. Saving must keep every unknown tag.
