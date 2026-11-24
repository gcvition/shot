Name=Close Orbit
Description=A sphere orbits around you at close range. Constant-turn tracking.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=OnTargetPercent
ShotSkillWeights=tracking=1.0
ShotSpawnRegion=Box min=(-1,1,-8) max=(1,2,-6)
ShotSpawnCount=1
ShotSpawnRadius=0.35
ShotSpawnRule=delay=0.3
ShotMotion=Orbit radius=3 speed=1.0 y=-0.2
