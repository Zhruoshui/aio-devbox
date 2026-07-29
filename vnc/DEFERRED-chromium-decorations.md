# Deferred: Chromium window decorations / buttons in the VNC pane

> Status: **reverted 2026-07-29** to the first working version. This doc records
> what was tried so the work can be resumed later. The VNC stack itself works
> (chromium drivable, CJK renders); only the window-decoration polish below was
> rolled back because it was incomplete and the user chose to defer it.

## What the user wanted

Match the AIO reference: the Chromium window in the VNC desktop should have
**no minimize / maximize / close buttons** and **not be draggable**, and closing
it should **not lose the page**. (AIO shows a button-less browser filling the
viewport.)

## The two real problems

1. **Chromium's window is draggable + closable, and closing it loses the page.**
   The WM (openbox) title bar carries the min/max/close buttons and the drag
   handle. Closing chromium made the original `wait -n` supervisor tear the whole
   container down (restart -> about:blank -> page lost).
2. **Minimizing loses the window.** Bare openbox ships **no panel/taskbar**, so a
   minimized window has nowhere to restore from; and chromium doesn't exit on
   minimize, so the supervisor never fires.

## What was tried (all reverted)

### A. openbox `decor=no` + `maximized=true` for Chromium — WORKED (WM side)
A minimal `vnc/openbox-rc.xml` with one application rule, COPY'd to
`/etc/aio/openbox-rc.xml` and copied to `~/.config/openbox/rc.xml` by the
entrypoint before openbox starts (the workspace volume shadows `/home/gem`, so
it can't be baked there):

```xml
<openbox_config xmlns="http://openbox.org/3.4/rc">
  <applications>
    <application class="Chromium">
      <decor>no</decor>
      <maximized>true</maximized>
    </application>
  </applications>
</openbox_config>
```

WM_CLASS class is `Chromium` (verified via `xprop`). Verified live: after
`openbox --reconfigure`, the chromium window flipped to
`_NET_WM_STATE = MAXIMIZED_VERT/HORZ + _OB_WM_STATE_UNDECORATED` and
`_NET_FRAME_EXTENTS = 0,0,0,0`. A minimal `<applications>`-only rc.xml
cold-starts openbox cleanly (tested a fresh openbox on a second X display); the
"Unable to find a valid menu file" log line is harmless.

**Result:** removed the WM title bar (no drag, no WM buttons) — but see C.

### B. Chromium auto-restart loop — WORKED
Ran chromium under `setsid bash -c 'while true; do chromium ...; sleep 1; done'`
so a chromium exit relaunches in-place, and made only Xvnc/openbox/websockify
`wait -n`-critical (chromium non-critical). Verified: `pkill -x chromium` ->
container stayed up, chromium relaunched, log showed "chromium exited,
relaunching". cleanup kills the setsid process group.

### C. `--disable-features=ClientSideDecorations` — DID NOT WORK (the blocker)
After A removed the WM title bar, Chromium **still drew its own** min/max/close
buttons in the top-right (client-side decorations, CSD) in bare-X11. Adding
`--disable-features=TranslateUI,ClientSideDecorations` to the launch did nothing
— the buttons stayed.

**Why (researched 2026-07-29 via web):** Chromium 150 on Linux draws its own CSD
in bare-X11 and this is **architecturally entangled** with the browser chrome —
there is **no working flag/policy/env to hide the CSD buttons** in v150.
`--disable-features=ClientSideDecorations` is not honored. The identical
unresolved problem exists in `jlesage/docker-baseimage-gui` (issue #161, April
2025): their `<decor>no</decor>` also fails to remove Chrome's buttons. The only
way to fully hide the buttons is **kiosk mode** (`--kiosk`), which also removes
the address bar (can't type URLs) — rejected by the user.

### D. Session-restore policy + tint2 taskbar — WORKED (the accepted workaround)
Since the buttons can't be hidden, made them non-destructive:
- **`/etc/chromium/policies/managed/aio-restore.json` = `{"RestoreOnStartup": 1}`**
  ("Continue where you left off"). Combined with B, closing chromium relaunches
  and restores the previous page. (RestoreOnStartup: 1 = continue; 4 = open URLs;
  5 = new tab page — confirmed via Chrome enterprise policy docs.)
- **`tint2` taskbar** (5th supervised process, restart loop) at the bottom: a
  minimized window shows as a task; click to restore. tint2 is a 30px
  `_NET_WM_WINDOW_TYPE_DOCK` with `_NET_WM_STRUT=30`; maximized chromium becomes
  1280x770, leaving the panel visible (not covered). No custom config needed —
  tint2 creates a default `~/.config/tint2/tint2rc` (panel items `LTSC`,
  includes the taskbar `T`).

User accepted this ("这套方案可以了") but then chose to revert everything to the
first version and defer.

## What was reverted (to re-apply, resume from here)

Files changed (all in `vnc/`):
- **`vnc/Dockerfile`**: add `tint2` to apt install (+ `&& tint2 -v` check); add
  `COPY vnc/openbox-rc.xml /etc/aio/openbox-rc.xml`; add the policy RUN
  (`mkdir -p /etc/chromium/policies/managed && printf '{"RestoreOnStartup": 1}\n' > .../aio-restore.json`).
- **`vnc/entrypoint.sh`**: copy `/etc/aio/openbox-rc.xml` to
  `~/.config/openbox/rc.xml` before `openbox --sm-disable`; run chromium in the
  `setsid` restart loop with `--disable-features=TranslateUI,ClientSideDecorations`;
  launch `tint2` in a `setsid` restart loop; make chromium+tint2 non-critical
  (cleanup kills their process groups; `wait -n` only fires on
  Xvnc/openbox/websockify).
- **`vnc/openbox-rc.xml`** (deleted): the minimal rc.xml above.

**Kept (do NOT revert — essential):** `app/services.toml` `vnc.url`
`?path=vnc/websockify` (noVNC connects 404 without it), the SingletonLock
cleanup in entrypoint.sh (container crash-loops on recreate without it),
`fonts-noto-cjk` in the Dockerfile (CJK renders), and the pty resize fix
(`app/src/pty.rs`, `app/src/routes/terminal.rs`, `web/src/panes/XtermPane.tsx`
— unrelated to chromium).

## Recommended path forward (when resuming)

1. **Re-apply D (taskbar + session restore)** as the baseline — it's the
   accepted workaround and fully works. Buttons stay visible but minimize/close
   are non-destructive.
2. **If a button-less look is required**, the only real option is **kiosk mode**
   (`--kiosk`), accepting no address bar — provide a start page with a search box
   / links for navigation, or drive via CDP (AIO uses CDP 9222 for agent-driven
   browsing; see research 08).
3. **Track upstream**: if Chromium re-adds a CSD-disable flag, re-try
   `--disable-features=ClientSideDecorations` (or its successor). Check
   `chrome://flags` in the running chromium and the crbug tracker.
4. **Alternative browsers**: Firefox under openbox respects `decor=no` (no CSD
   buttons) — a Firefox-in-VNC container would hide buttons cleanly. Out of
   scope for the chromium-based MVP but worth noting.

## Pointers / evidence

- Research: `.trellis/tasks/archive/2026-07/07-28-research-aio-architecture/research/08-server-architecture-reverse-engineered.md`
  (AIO VNC stack: openbox + `agent-browser-init.sh` + `browser-supervisor.py`,
  CDP 9222).
- Journal: `.trellis/workspace/ruoshui/journal-1.md` (2026-07-29 entry — full
  investigation + verification logs).
- implement.md "Risky points / rollback" has the noVNC-path and SingletonLock
  notes (kept); the CSD/decoration notes were removed with the revert.
