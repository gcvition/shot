Name=Switch 3 Bots TTK
Description=Three strafing humanoids, 150 HP each. Spray-transfer between them; score counts kills.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=switching
ShotScoring=Kovaak
ShotSkillWeights=flick=0.5;recoil=0.5
ShotWeapon=rifle rpm=800 dmg=20 mag=40
ShotSpawnRegion=Box min=(-5,1.4,-14) max=(5,1.4,-8)
ShotSpawnCount=3
ShotSpawnShape=humanoid
ShotSpawnHP=150
ShotSpawnRule=minSep=1.5 delay=0.5 maxAlive=3
ShotMotion=Strafe speed=1.2..2 reverse=1..2 accel=20
