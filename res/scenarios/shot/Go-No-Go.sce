Name=Go-No-Go
Description=Kill the bright spheres, never touch the dark red ones. Discipline under speed.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=flick
ShotScoring=LegacyHits
ShotSkillWeights=flick=0.6;reaction=0.4
ShotSpawnRegion=Box min=(-5,1,-15) max=(5,2.5,-8)
ShotSpawnCount=3
ShotSpawnRadius=0.28
ShotSpawnRule=minSep=0.8 delay=0.2..0.5
ShotSpawnRegion2=Box min=(-5,1,-15) max=(5,2.5,-8)
ShotSpawnCount2=2
ShotSpawnRadius2=0.28
ShotNoShoot2=1
ShotSpawnRule2=minSep=0.8 delay=0.6..1.2
