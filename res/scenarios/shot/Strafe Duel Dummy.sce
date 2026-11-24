Name=Strafe Duel Dummy
Description=Single ADAD humanoid that does not shoot back. One headshot or four body shots.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=duel
ShotScoring=Kovaak
ShotSkillWeights=flick=0.4;tracking=0.6
ShotWeapon=rifle rpm=750 dmg=25 hsmult=4 mag=30
ShotSpawnRegion=Box min=(-3.5,1.4,-12) max=(3.5,1.4,-9)
ShotSpawnCount=1
ShotSpawnShape=humanoid
ShotSpawnHP=100
ShotSpawnRule=delay=0.3 maxAlive=1
ShotMotion=Dodge speed=1.8..2.4 react=1.0
