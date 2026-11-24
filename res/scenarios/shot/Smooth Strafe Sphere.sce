Name=Smooth Strafe Sphere
Description=A sphere strafes left-right at mid range. Keep the crosshair glued to it. Tracking intro.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=OnTargetPercent
ShotSkillWeights=tracking=1.0
ShotSpawnRegion=Box min=(-5,1.2,-14) max=(5,2.4,-8)
ShotSpawnCount=1
ShotSpawnRadius=0.35
ShotSpawnRule=delay=0.3
ShotMotion=Strafe speed=2.5..3.5 reverse=0.8..1.6 accel=24 symmetric=1
