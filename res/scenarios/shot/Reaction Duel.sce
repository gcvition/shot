Name=Reaction Duel
Description=A bot appears after a short delay and fires back. Land the headshot before it lands its shot.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=duel
ShotScoring=Kovaak
ShotSkillWeights=reaction=0.6;flick=0.4
ShotPlayer=preset=static hp=100
ShotWeapon=hitscan mode=semi dmg=100
ShotSpawnRegion=WallRect center=(0,1.6,-11) size=(4,1,0)
ShotSpawnCount=1
ShotSpawnShape=humanoid
ShotHeadshotOnly=1
ShotBot=aim=0.5,1.0 interval=1.4 acc=0.6 dmg=34
ShotSpawnRule=delay=0.8..1.5 maxAlive=1
ShotMotion=Static
