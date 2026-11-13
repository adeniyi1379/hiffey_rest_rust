# Hiffey POS pilot checklist

## What this build does

- Terminal A hosts the till: axum server, SQLite database and the UI in one desktop app.
- Terminal B is UI only and talks to Terminal A over the shop LAN.
- Everything is written locally first: orders, payments, stock moves, audit and a sync outbox.
- Screens: setup, PIN login, sale, held orders, stock, menu, workers, settings, sync status.

## Run it

Terminal A:

```powershell
cd crates\pos-app\src-tauri
$env:HIFFEY_SERVER = "1"
cargo tauri dev
```

The server listens on `0.0.0.0:47800` and stores the database in the app data folder.
Override with `HIFFEY_BIND` and `HIFFEY_DB` if needed.

Terminal B:

```powershell
cd crates\pos-app\src-tauri
cargo tauri dev
```

On the Connection screen enter `http://<terminal-a-ip>:47800`, test, and save.

Browser preview without Tauri:

```powershell
cargo run -p pos-server --bin dev-server
cd crates\pos-ui
trunk serve
```

Then open `http://127.0.0.1:8080`.

Before the pilot: give Terminal A a static LAN IP and allow TCP 47800 through Windows Firewall.

## One-time setup on Terminal A

1. Fill shop name, owner name, address and phone.
2. Write down the owner PIN shown once.
3. Menu: add items with unit and price; create packs if the kitchen sells combos.
4. Stock: record an opening batch for each item sold in units.

## Smoke test before service

- [ ] Terminal B reaches `/health` and signs in against Terminal A.
- [ ] Cashier PIN works; five wrong PINs lock the till for two minutes.
- [ ] Add items, set table for dine in, hold the ticket, resume it, take cash payment.
- [ ] Takeaway order includes the packaging fee from Settings.
- [ ] Change due is correct and the paid docket shows the order number.
- [ ] Sold-out and low-stock tiles reflect stock after payment.
- [ ] Restock, wastage and stock count produce the expected balances.
- [ ] Manager/owner PIN is required for a paid-order void; cashier cannot void.
- [ ] Restart Terminal A mid-day: sales and stock are still there.
- [ ] Reports shows the day's totals, payment split and item mix; both CSV files download.
- [ ] 1280 x 800 screen readable, all keys reachable by touch.

## Known gaps for the pilot

- Print jobs queue but no printer agent runs yet; the paid screen says so.  
- Reports covers sales, payments, item mix and CSV export; discounts are not implemented.
- Paid-order void and order search have server support but no full UI.
- Cloud sync is write-only and switched off for the pilot.
- Database backups are made on server start only, under the app data `backups` folder.
