Name=Arc Lead
Description=Lobbed projectiles with gravity at long range. Arc and lead at once.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=LegacyHits
ShotSkillWeights=projectile=1.0
ShotWeapon=lob speed=35 grav=1 dmg=50
ShotSpawnRegion=Box min=(-6,1.2,-26) max=(6,2.5,-18)
ShotSpawnCount=2
ShotSpawnRadius=0.35
ShotSpawnRule=delay=0.4 maxAlive=2
ShotMotion=Strafe speed=1.5..3 reverse=1..2 accel=24
