# Whisple licensing

Licensing and the free trial are opt-in at compile time. `cargo build --release`
and `./scripts/package-macos.sh` build an unrestricted app with no trial,
license checks, or License page. To build with the commercial license flow, use
`cargo build --release --features licensing` or
`./scripts/package-macos.sh --with-licensing`. The macOS release workflow uses
the licensed build explicitly.
Unlicensed builds do not auto-install release binaries; their update actions
open the release notes, and users can rebuild from source when they want an update.

Whisple validates its license directly against Polar's public customer-portal
license-key endpoints. The desktop app does not contain an organization access
token or need a Whisple backend.

## Polar configuration

- Organization: `whisple` (`9c7736ba-52be-460e-ae28-a9c5bc2e5b26`)
- Lifetime product: `3b23cb9a-b14a-4625-b8b3-9685c18f694c`
- License Keys benefit: `41af04ce-d991-4575-af1d-b216fe69ce2d`
- The benefit currently permits five device activations and is attached to the
  Lifetime product. Customers can release activations in Polar's portal.

These are public identifiers. Keep organization API tokens out of the desktop
app. If the benefit is replaced, update `BENEFIT_ID` in `src/licensing/mod.rs` and
verify the product grants the replacement before distributing the build.

## App behavior

The three-day free trial starts on first launch (including onboarding). Its
start time and latest observed time are stored in a separate system credential
entry. It allows dictation without a key for exactly 72 hours, including
offline use. The deadline is checked when dictation is requested, not just at
startup. Restarting the app, activating a key, or deactivating one does not
restart the trial. Clock rollback beyond five minutes from the last saved
check blocks trial access until the clock is corrected; moving the clock
forward past the deadline expires the trial. The trial is local to this
credential store, not a server-verified per-person entitlement. Removing the
credential store entry or reinstalling onto a new device cannot be prevented
by this client-only design.

The app activates a pasted key once, then stores the key and activation ID in
the user's system credential store, separately from the trial. It validates
the key on startup and every six hours. Paid dictation requires a granted
license from this organization and benefit, plus a matching activation ID.
A successful check allows up to 72 hours of temporary paid offline use,
ending sooner if the key expires. Revoked, disabled, expired, or mismatched
keys lose paid access immediately on the next online check. Deactivation
calls Polar before removing only the license credential. If the trial has not
ended, deactivating returns the app to trial access. A saved paid key that
fails validation does not block remaining trial time: the License page still
shows its validation issue, but dictation uses the trial until it expires.

Whisple costs $19 once after the trial (`PRICE` in `src/licensing/mod.rs`). Change
that constant together with the Polar product price and the website.

When the trial ends, nothing opens by itself. The voice bar says "Free trial
ended" and replaces the model capsule with "Unlock · $19", which opens the
License page. Pressing record on a locked bar also opens the License page.
On the trial's last day the bar shows the time left next to the gear.

The License page shows the time left ("2 days 4 hours left") or explains that
the trial has ended. It links to the hosted Polar checkout and customer portal.
The current Polar product remains a one-time Lifetime purchase; this three-day
trial is local and is not configured in Polar.

## Release check

Use a test purchase or a key explicitly allocated for testing to activate on
a device, restart the app, confirm recording works, and deactivate. Then test
an invalid key, a full device limit, revoked access, and a temporary network
outage. The public endpoints and expected fields are documented in
[Polar's license-key guide](https://polar.sh/docs/features/benefits/license-keys).
