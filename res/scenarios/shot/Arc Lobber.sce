Name=Arc Lobber
Description=Projectile arcs with explosive splash. Lead the shot and land the blast near strafing targets.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=click
ShotScoring=Kovaak
ShotSkillWeights=flick=0.5;tracking=0.5
ShotWeapon=projectile speed=28 gravity=1.0 radius=0.12 dmg=100 aoe=1.6 mag=4 reload=2.0
ShotSpawnRegion=Box min=(-5,0.2,-18) max=(5,2.6,-12)
ShotSpawnCount=4
ShotSpawnRule=minSep=1.2 minAng=6 maxAlive=4
ShotMotion=Strafe speed=1.5..3.0 reverse=0.9..1.8 accel=20
