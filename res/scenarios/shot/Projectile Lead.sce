Name=Projectile Lead
Description=Slow projectiles vs a strafing sphere. Lead the target; watch travel time.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=tracking
ShotScoring=LegacyHits
ShotSkillWeights=projectile=0.7;tracking=0.3
ShotWeapon=projectile speed=30 dmg=50
ShotSpawnRegion=Box min=(-4,1.2,-16) max=(4,2.5,-10)
ShotSpawnCount=2
ShotSpawnRadius=0.3
ShotSpawnRule=delay=0.4 maxAlive=2
ShotMotion=Strafe speed=2..3.5 reverse=1..2 accel=24
