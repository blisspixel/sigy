# 0030: Refresh one directory page on a saved policy

Date: 2026-09-22. Status: implemented for one bounded page. A complete catalog and the globe stay open.

## Decision

`radio policy set` stores one rule: one directory request and an interval from 1 to 168 hours. The service checks that rule locally about once a second and admits the current slot when it is due. The first slot waits one full interval. A missed slot is not backfilled. A slot that completed, failed, or was interrupted is not fetched again. Restart does not resume a running refresh. Changing the rule starts a new interval and does not rewrite refresh history.

The admission path is the existing directory refresh. One refresh runs at a time. The request does not open a station stream and does not send a click. Saving the rule, searching, showing status, and opening a client do not fetch. A failed refresh leaves the last usable cache. Favorites stay, and unseen stations stay, because one page cannot prove that the directory removed them.

## Consequences

Catalog schema is v21. Local IPC is v22. Stop an older service before replacing its binary. This does not claim a complete station catalog, and it does not exit stage 4.
