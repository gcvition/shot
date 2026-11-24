Name=Corner Clear
Description=Peek out from behind the block with A/D; a head target pops at one of the corner spots. First-shot speed and placement.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=peek
ShotScoring=ReactionMs
ShotSkillWeights=peek=0.8;flick=0.2
ShotPlayer=pos=(0,0,-2) preset=tactical hp=100
ShotWeapon=hitscan mode=semi dmg=100
ShotBlock1=-0.6,0,-7,0.6,3,-6
ShotSpawnRegion=Box min=(-3.5,1.4,-13) max=(3.5,1.4,-9)
ShotSpawnCount=1
ShotSpawnShape=humanoid
ShotHeadshotOnly=1
ShotSpawnRule=delay=0.8..1.6 maxAlive=1
ShotMotion=Static
