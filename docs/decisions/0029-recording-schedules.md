# 0029: Record one station by civil time

Date: 2026-09-22. Status: implemented for one source rule. Topic monitors and the globe stay open.

## Decision

`schedule create` stores one rule: one source revision, one IANA time zone, and a once, daily, or weekly civil clock. Each occurrence has a finite duration and a finite byte budget. Only the next occurrence is materialized. The service checks about once a second and admits that occurrence when the window is open and a decoder file is configured. The admitted plan keeps the civil start, end, and byte budget. A later edit of the rule does not rewrite it.

Civil resolution uses the IANA data bundled with jiff 0.2.37, reviewed on 2026-09-22. The service does not read the operating-system zoneinfo database. A window that has already ended is missed and is not backfilled. A civil time that does not exist is missed as a spring-forward. A civil time that occurs twice uses the earlier offset, once. Starting after the civil start records a prefix gap from the planned beginning and does not create a second job. Restart does not admit that occurrence again. No analysis profile can be stored on the rule.

## Consequences

Catalog schema is v20. Local IPC is v21. Stop an older service before replacing its binary. This does not refresh the station cache on its own, and it does not exit stage 4.
