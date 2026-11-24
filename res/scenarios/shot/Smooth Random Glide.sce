Name=Smooth Random Glide
Description=A sphere drifts with smooth random motion (Ornstein-Uhlenbeck). Unpredictable tracking.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=OnTargetPercent
ShotSkillWeights=tracking=1.0
ShotSpawnRegion=Box min=(-6,1,-16) max=(6,3,-8)
ShotSpawnCount=1
ShotSpawnRadius=0.35
ShotSpawnRule=delay=0.3
ShotMotion=SmoothRandom speed=3.5 tau=0.4
