# Platform support

What accshift switches today, what could be built, and what will not happen on a
given OS. The README lists only the integrations that ship; this page is the
full grid, roadmap included.

Support is per operating system, not per platform: a launcher that keeps its
session in a file accshift can snapshot works, and one that binds the session to
a machine-level secret accshift cannot reach does not.

## Grid

| Platform                                                | Windows         | macOS           | Linux           |
| ------------------------------------------------------- | --------------- | --------------- | --------------- |
| Steam                                                   | Done            | Done            | Done            |
| Riot Games (Valorant, League of Legends, TFT)           | Done            | Possible        | Not feasible    |
| Battle.net (Overwatch 2, Diablo IV, WoW, Call of Duty)  | Done            | Done            | Not feasible    |
| Epic Games (Fortnite, Rocket League)                    | Done            | Possible        | Possible        |
| Ubisoft Connect (Rainbow Six Siege, The Division 2)     | Done            | Possible        | Possible        |
| Roblox                                                  | Done            | Possible        | Possible        |
| GOG Galaxy (Cyberpunk 2077, The Witcher 3)              | Need testing    | Possible        | Not feasible    |
| Jagex Launcher (RuneScape, Old School RuneScape)        | Need testing    | Possible        | Not feasible    |
| Discord                                                 | Need testing    | Possible        | Possible        |
| EA app (Apex Legends, The Sims 4, Battlefield)          | Possible        | Possible        | Not feasible    |
| Rockstar Launcher (GTA V, Red Dead Redemption 2)        | Possible        | Not feasible    | Possible        |
| GeForce Now                                             | Possible        | Possible        | Possible        |
| HoYoverse / HoYoPlay (Genshin Impact, Honkai Star Rail) | Possible        | Not feasible    | Not feasible    |
| Minecraft Launcher                                      | Possible        | Possible        | Possible        |

## What the states mean

`Done`
GUI and CLI implemented, and verified on that OS. This is what the README
advertises.

`Need testing`
Implemented and believed working, but not yet confirmed on enough machines to
be called Done. Bug reports on these are especially useful.

`Possible`
The launcher stores its session somewhere accshift could snapshot, so the
integration is realistic, but no code exists yet. Nothing here is scheduled.
Priority follows what users actually ask for, so an issue is the way to move
one of these up.

`Not feasible`
Not realistic on that OS, usually because the launcher does not run there, or
because it binds the session to something accshift cannot capture and restore.
These are not waiting on effort.

## Asking for a platform

Open a [platform request](https://github.com/klNuno/accshift/issues/new/choose)
and say which launcher, which OS, and how you use it. A request with a real use
case behind it is what turns a `Possible` row into work.

If a `Need testing` row matches your setup, reporting that it worked is worth as
much as reporting that it broke: those rows move to `Done` on evidence, not on
time passing.
