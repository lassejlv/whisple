# Whisple licensing

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
app. If the benefit is replaced, update `BENEFIT_ID` in `src/license.rs` and
verify the product grants the replacement before distributing the build.

## App behavior

The app activates a pasted key once, then stores the key and activation ID in
the user's system credential store. It validates the key on startup and every
six hours. Dictation requires a granted license from this organization and
benefit, plus a matching activation ID. A successful check allows up to 72
hours of temporary offline use, ending sooner if the key expires. Revoked,
disabled, expired, or mismatched keys lose access immediately on the next
online check. Deactivation calls Polar before removing the local credential.

The License page links to the existing hosted Polar checkout and to the
Whisple customer portal. No local 14-day trial is implemented: the current
Polar product is a one-time Lifetime purchase with no trial configured.

## Release check

Use a test purchase or a key explicitly allocated for testing to activate on
a device, restart the app, confirm recording works, and deactivate. Then test
an invalid key, a full device limit, revoked access, and a temporary network
outage. The public endpoints and expected fields are documented in
[Polar's license-key guide](https://polar.sh/docs/features/benefits/license-keys).
