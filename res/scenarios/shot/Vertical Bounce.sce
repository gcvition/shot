Name=Vertical Bounce
Description=A sphere bounces vertically ahead of you. Vertical tracking.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=OnTargetPercent
ShotSkillWeights=tracking=1.0
ShotSpawnRegion=Box min=(-3,0.3,-14) max=(3,0.3,-8)
ShotSpawnCount=1
ShotSpawnRadius=0.35
ShotSpawnRule=delay=0.3
ShotMotion=Jumper speed=5..7 gravity=14 interval=0.8..1.6
