# Third-party notices

## CPA Usage Keeper

The first version of this web console adapts page structure, visual components,
time-range controls, charts, request-event virtualization, column preferences,
details, and export interactions from
[CPA Usage Keeper](https://github.com/Willxup/cpa-usage-keeper).

The adapted code was vendored into this repository in project commit `32876bf`.
It has since been narrowed to my-ai-gateway's Overview, Analysis, and Request
Events product surface and its PostgreSQL Usage API semantics. CPA Usage
Keeper's Go backend, SQLite storage, Redis queue, Management API, credentials,
quota, pricing, ranking, and request-log body access are not used by the active
web application.

CPA Usage Keeper is distributed under the MIT License. The complete license
text is preserved in [`licenses/CPA_USAGE_KEEPER_LICENSE`](licenses/CPA_USAGE_KEEPER_LICENSE).

Copyright (c) 2026 Will
