Name=Counter-Strafe Drill
Description=Tactical preset: strafe A/D then counter-strafe to stop fast and fire at the popping head. Speed matters - fire when stopped.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=movement
ShotScoring=Kovaak
ShotSkillWeights=movement=0.6;flick=0.4
ShotPlayer=preset=tactical hp=100
ShotWeapon=hitscan mode=semi dmg=50 spread=0,0.4,3,2
ShotSpawnRegion=Box min=(-4,1.7,-12) max=(4,1.7,-10)
ShotSpawnCount=2
ShotSpawnShape=humanoid
ShotHeadshotOnly=1
ShotSpawnRule=minSep=2 delay=0.5..1.2 maxAlive=2
ShotMotion=Static
