Name=1v3 Arena
Description=Three bots with long TTK cycle in. Manage the angles, use movement, survive.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=duel
ShotScoring=Kovaak
ShotSkillWeights=duel=1.0
ShotPlayer=preset=arena hp=200
ShotWeapon=rifle rpm=800 dmg=20 hsmult=2 mag=40
ShotSpawnRegion=Box min=(-5,1.4,-14) max=(5,1.4,-8)
ShotSpawnCount=3
ShotSpawnShape=humanoid
ShotSpawnHP=200
ShotBot=aim=0.5,1.2 interval=1.3 acc=0.4 dmg=7 strafe=1
ShotSpawnRule=minSep=1.5 delay=1.0 maxAlive=3
ShotMotion=Strafe speed=1.5..2.5 reverse=1..2 accel=22
