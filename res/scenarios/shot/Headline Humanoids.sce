Name=Headline Humanoids
Description=Five standing humanoids at varied distances. Headshots only — body shots count as misses.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=flick
ShotScoring=LegacyHits
ShotSkillWeights=flick=0.6;micro=0.4
ShotSpawnRegion=Box min=(-6,1.7,-24) max=(6,1.7,-10)
ShotSpawnCount=5
ShotSpawnShape=humanoid
ShotHeadshotOnly=1
ShotSpawnRule=minSep=2 delay=0.3 maxAlive=5
ShotMotion=Static
