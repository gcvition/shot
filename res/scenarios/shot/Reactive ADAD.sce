Name=Reactive ADAD
Description=A dodging target that changes direction when you shoot at it. Track it through the jukes.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=OnTargetPercent
ShotSkillWeights=tracking=0.7;reaction=0.3
ShotSpawnRegion=Box min=(-6,0.4,-16) max=(6,0.4,-10)
ShotSpawnCount=1
ShotSpawnRadius=0.35
ShotSpawnRule=delay=0.3
ShotMotion=Dodge speed=4 accel=40 gravity=20 jump=6 lr=0.3..0.9 fb=0.6..1.4 jumpi=4..8 band=6..16 react=0.8 reactdelay=0.1..0.25 reactcd=0.4
