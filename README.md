# Hiffey POS

Hiffey POS is a Windows desktop point-of-sale for a restaurant. One PC (the **host**) runs the
Axum server, SQLite database and till UI; additional PCs (**clients**) run the same UI-only app and
connect to the host over the shop LAN. All data is stored locally first — cloud sync is disabled for
the current client test.

Current client-test build: **0.3.3**.

## What this build includes

- One NSIS installer for both roles. The first launch shows an in-app **role screen**:
  **Client + Server** (host and sell) or **Client** (connect to the host). Choosing a server role
  starts the embedded server immediately.
- Clean-install vs upgrade prompt when reinstalling over a PC that already has shop data. The
  default is **upgrade (keep data)**; silent installs never delete data.
- Touch-friendly till with categories, hold/resume, dine-in tables, takeaway packaging fee, cash
  tender with change, and split cash/transfer/card payments.
- **Dual-screen customer display** for a second monitor on the same PC: idle branding, running
  ticket, payment/change, and paid/thank-you states. It is display-only and driven by in-process
  Tauri events.
- Manager transaction history with date/status/search filters, order detail, duplicate reprints and
  authorised paid-order voids.
- Owner analytics on Reports: net sales, average ticket, items sold, voided totals, sales-by-day
  trend, busiest hours, and breakdowns by cashier, category, item and payment method.
- Owner-only worker management; cashiers cannot reach Workers.
- Windows thermal printing with a per-PC printer setting, test print, automatic receipt printing,
  serving slips, duplicate labels and visible job status/waiting feedback.
- Reserved **Packs** category handling (created once; cannot be created again or appear twice),
  collapsible sidebar with persisted state, and a layout that scrolls correctly on 1366 × 768
  screens.

## Requirements

For a two-terminal client test:

- Two Windows 10/11 PCs on the same router or switch.
- A fixed/reserved LAN address on the host, for example `192.168.0.10`.
- TCP **47800** allowed through Windows Firewall on the host.
- Optional: a 58 mm or 80 mm thermal printer on any PC that prints.
- Optional: a second monitor (or a tool such as spacedesk) for the customer display.

> Use disposable test data. `data/dev.db` is throwaway development data and is never client data.

## Install and first run

1. Run `Hiffey POS_<version>_x64-setup.exe` on the PC that will keep the shop data.
2. On the **Set up this computer** screen choose **Client + Server**, then **Continue**.
   The server starts straight away and the database is created.
3. Run the same installer on the second PC and choose **Client**. On the Connection screen enter
   the host's real LAN address, for example `http://192.168.0.10:47800`, tap **Test**, then **Save**.

`127.0.0.1` and `127.0.0.x` are loopback addresses: they always point at the local PC and cannot be
used by a client to reach the host. Use the host's IPv4 LAN address.

If you reinstall over existing data, the installer asks whether to **delete it and start clean**
(Yes) or **keep it** (No, default). Silent installs always keep data. A clean install returns the PC
to the first-run role screen.

One-time shop setup on the host: fill in shop details, write down the owner PIN shown once, add menu
categories/items, and record opening stock.

## Customer display

On the host PC, attach or enable the second monitor before configuring the display.

1. Go to **Settings → Second display** (the panel is at the top of Settings).
2. Set **Customer display** to **On (second monitor)**.
3. Pick the monitor, or leave **Choose automatically**.
4. Tap **Save display**. The customer window opens on the selected monitor.
5. Tap **Show a test ticket** to confirm the layout.

The customer window updates with the open ticket and running total, tender and change, and a
paid/thank-you screen. Closing the customer window only hides it; closing the main POS destroys it.
The setting is stored per PC and survives restarts.

## Printing

1. Go to **Settings → Receipt printer**.
2. Choose the printer and paper width (80 mm or 58 mm).
3. Sign in as owner if a printing key is required, then tap **Save printer**.
4. Tap **Print a test receipt**.

Receipts queue automatically after payment. Serving slips can be sent from the sale screen
(**Print slip**, which leaves the ticket open), the **Payment taken** card, or History
(**Reprint slip** for non-voided orders). The UI reports printed, failed with reason, or still
waiting. Hardware smoke testing is still part of the client test.

## Repository layout

| Crate | Role |
|---|---|
| `crates/pos-core` | Pure money, pricing and order-number logic. No I/O. |
| `crates/pos-types` | Shared serde DTOs used by server and UI. |
| `crates/pos-server` | Axum API, SQLite via `sqlx`, forward-only migrations, `dev-server` binary. |
| `crates/pos-print` | Receipt/ESC-POS rendering and Windows spooler support. |
| `crates/pos-ui` | Leptos/WASM CSR front end; Tailwind v4 built with `npm run css:build`. |
| `crates/pos-app/src-tauri` | Desktop host: role screen, embedded server, printer agent and NSIS installer hook. |

Dependency direction: `pos-ui` → `pos-types`/`pos-core`; `pos-server` → `pos-types`/`pos-core`/
`pos-print`; `pos-app` → `pos-server`/`pos-print`. Keep `pos-core` free of I/O and async.

Important invariants:

- Money is integer kobo. Never use floats for money.
- Migrations are forward-only (`crates/pos-server/migrations/`); never edit an applied migration.
- `print_jobs.status` allows only `queued|printed|failed`. Claiming a job sets lease fields only.
- **Packs** is a reserved category name and must not be seeded or created by users.
- Worker management is owner-only.
- Cloud sync, VAT, discounts and refund semantics are Version 2 work and are not implemented.

## Development

`cargo` may not be on PATH in every shell. If needed:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

If `target/` is locked by a running app, use a scratch target directory instead of killing it:

```powershell
$env:CARGO_TARGET_DIR = "$env:USERPROFILE\AppData\Local\Temp\opencode\hiffey-build"
```

### CI checks

Run all of these before calling a change done:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Browser preview (no Tauri)

```powershell
cargo run -p pos-server --bin dev-server
# in a second terminal:
cd crates\pos-ui
npm ci
trunk serve
```

Open `http://127.0.0.1:8080`.

### Desktop installer

```powershell
cd crates\pos-app\src-tauri
cargo tauri build
```

The NSIS installer is written to `target\release\bundle\nsis` (or under `CARGO_TARGET_DIR` when
set). `HIFFEY_SERVER=1` forces host mode and skips the first-run role screen for development;
`HIFFEY_BIND` and `HIFFEY_DB` override the server bind address and database path.

## More documentation

- [CLIENT-TEST.md](./CLIENT-TEST.md) — client test script and expected results.
- [PILOT.md](./PILOT.md) — run and smoke-test checklist.
- [DEVELOPMENT_PLAN.md](./DEVELOPMENT_PLAN.md) — phased plan, requirement mapping and CI checks.
- [AGENTS.md](./AGENTS.md) — repository working notes and invariants.
