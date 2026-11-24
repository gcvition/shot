Name=Reflex Pop
Description=One target pops at a time and vanishes quickly. Kill it before it expires.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=reaction
ShotScoring=ReactionMs
ShotSkillWeights=reaction=1.0
ShotSpawnRegion=AngularShell yaw=-45..45 pitch=-5..15 dist=8..14
ShotSpawnCount=1
ShotSpawnRadius=0.3
ShotSpawnRule=delay=0.5..1.5 life=1.2..1.8 minAng=15
