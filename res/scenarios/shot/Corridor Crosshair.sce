Name=Corridor Crosshair
Description=Walk the corridor with W; targets appear at door openings on both sides. Hold crosshair at head height on the corner.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=peek
ShotScoring=ReactionMs
ShotSkillWeights=peek=0.7;reaction=0.3
ShotPlayer=pos=(0,0,-2) preset=tactical hp=100
ShotWeapon=hitscan mode=semi dmg=100
ShotMapBounds=-8,0,-30,8,4,0
ShotBlock1=-1.5,0,-14,-0.5,3,-6
ShotBlock2=0.5,0,-14,1.5,3,-6
ShotSpawnRegion=Box min=(-4,1.4,-24) max=(4,1.4,-16)
ShotSpawnCount=3
ShotSpawnShape=humanoid
ShotSpawnRule=minSep=1.5 delay=0.6..1.4 maxAlive=3 los=1
ShotMotion=Static
