Name=ADAD Duel
Description=Strafing duel - both sides move. Track the ADAD humanoid and kill it before it kills you.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=duel
ShotScoring=Kovaak
ShotSkillWeights=tracking=0.4;flick=0.3;movement=0.3
ShotPlayer=preset=tactical hp=100
ShotWeapon=rifle rpm=750 dmg=25 hsmult=4 mag=30
ShotSpawnRegion=Box min=(-3.5,1.4,-12) max=(3.5,1.4,-9)
ShotSpawnCount=1
ShotSpawnShape=humanoid
ShotSpawnHP=100
ShotBot=aim=0.4,0.9 interval=0.9 acc=0.45 dmg=8 strafe=1
ShotSpawnRule=delay=0.8 maxAlive=1
ShotMotion=Dodge speed=1.8..2.6 react=0.8
