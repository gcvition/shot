Name=Moving Clicks
Description=Strafing spheres; click to kill. Click-timing on moving targets.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=flick
ShotScoring=LegacyHits
ShotSkillWeights=flick=0.5;tracking=0.5
ShotSpawnRegion=Box min=(-5,1,-16) max=(5,2.5,-8)
ShotSpawnCount=4
ShotSpawnRadius=0.25
ShotSpawnRule=minSep=1 delay=0.2..0.5
ShotMotion=Strafe speed=1.5..3 reverse=0.6..1.4 accel=20
