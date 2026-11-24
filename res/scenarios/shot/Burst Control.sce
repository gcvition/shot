Name=Burst Control
Description=3-round bursts on a far humanoid. Reset between bursts; first-shot accuracy matters.
Timelimit=60.0
IsChallenge=true

[Shot Extension]
ShotVersion=1
ShotUnits=legacy
ShotCategory=recoil
ShotScoring=Kovaak
ShotSkillWeights=recoil=0.6;flick=0.4
ShotWeapon=burst mode=burst burstn=3 dmg=25 mag=24
ShotSpawnRegion=WallRect center=(0,1.7,-20) size=(6,3,0)
ShotSpawnCount=1
ShotSpawnShape=humanoid
ShotSpawnHP=150
ShotSpawnRule=delay=0.3 maxAlive=1
ShotMotion=Static
