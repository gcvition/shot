Name=Strafe and Shoot
Description=Free WASD movement on a platform; shoot strafing targets while moving. Arena preset, no accuracy penalty for moving.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=movement
ShotScoring=Kovaak
ShotSkillWeights=movement=0.5;tracking=0.5
ShotPlayer=preset=arena hp=100
ShotWeapon=rifle rpm=900 dmg=15 mag=60 spread=0.1,0.05,8,0.6
ShotSpawnRegion=Box min=(-5,1.2,-14) max=(5,2.5,-8)
ShotSpawnCount=4
ShotSpawnRadius=0.3
ShotSpawnHP=60
ShotSpawnRule=minSep=1.2 delay=0.4 maxAlive=4
ShotMotion=Strafe speed=2..4 reverse=0.8..1.8 accel=26
