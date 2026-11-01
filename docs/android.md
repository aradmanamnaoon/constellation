# Android status and release requirements

**There is no verified installable Constellation Android APK.** The desktop
interface and Rust crates do not, by themselves, establish an Android-native
build. No emulator, physical Android device, target-SDK lifecycle test,
runtime permission walkthrough, notification flow or release signing has
been completed for this project checkpoint.

The intended companion is a touch-friendly, locally safe experience for
rooms, messages, model selection, project evidence, encrypted recovery and
remote runner status. Desktop-only filesystem/process/Git actions need
explicit mobile capability checks and should be disabled or routed through
an authenticated, approved user-owned runner. Android background execution
is restricted by platform policies; it cannot promise perpetual local task
execution. Native file pickers, notifications, secure key storage, device
pairing and cancellation must be implemented and tested separately.

Before distributing an APK, establish a complete Android build toolchain and
release keystore handling, construct an actual APK with a unique application
ID, test install/upgrade/removal on supported emulator API levels and a real
device where available, and run touch targets, screen reader, right-to-left
text, permissions, background/foreground service, offline resume, approval
and encrypted-data recovery fixtures. The signed APK needs a fixed Git
source revision, versioned checksums, SBOM/license review and verified public
release provenance. Do not claim parity from a desktop mockup.

The exact release blockers remain in **P19** and **P20** of
[`docs/mission/active-task-plan.md`](mission/COMPLETE_EXECUTION_PLAN.md).
