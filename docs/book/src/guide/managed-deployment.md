# Managing Torno in an organization

An administrator can set some Torno settings for everyone who uses a computer, with Group
Policy or Microsoft Intune. A setting under policy shows in Preferences as **Set by your
organization**, and people cannot change it. While any policy is set, Torno says it is
**Managed by** the organization. This works on Windows today.

A computer without a policy behaves exactly as before. Torno only reads the policy: it
writes nothing, and it sends nothing about the policy anywhere.

## Install

The Windows installer (`torno-windows-x64.msi`, also arm64 and x86) installs for all users.
`msiexec /i torno-windows-x64.msi /qn` installs it without questions, so a deployment tool
can run it.

## The policies

| Policy | Registry value | Type | Values |
|---|---|---|---|
| Check for updates | `CheckForUpdates` | `REG_DWORD` | `1`: checks at start and once a day, and people cannot turn them off. `0`: no checks at all, as with `TORNO_UPDATE_CHECK=0` ([Updates and privacy](updates.md)). |
| Organization name | `OrganizationName` | `REG_SZ` | The name shown as **Managed by** and the name, up to 64 characters on one line. It changes no setting. |

The values live in the key `Software\Policies\Torno`:

- under `HKEY_LOCAL_MACHINE` for everyone on the computer, or
- under `HKEY_CURRENT_USER` for one person.

When both hold a value, the computer's value wins. A value of another type, a number other
than the ones listed, or an empty name counts as not set.

Torno reads the policy when it starts. A change applies the next time Torno starts.
When a policy is removed, the person's own earlier choice is back.

The same value names under `Software\Policies\Torno\Recommended` are reserved for defaults
that people may change. Torno reads them and does not apply them yet.

## The templates

Each release has `torno-<version>-windows-policy-templates.zip`, listed in
`SHA256SUMS.txt`. It holds the administrative template, `torno.admx`, and its English text,
`en-US/torno.adml` (they are also in `packaging/windows/policies/` in the source). Take the
zip from the release you deploy: it holds the policies that version reads. The policies appear in
the **Torno** folder.

**Group Policy.** Copy `torno.admx` into `PolicyDefinitions` and `torno.adml` into
`PolicyDefinitions\en-US`, on one computer (`%SystemRoot%\PolicyDefinitions`) or in the domain's
central store. The policies are under both Computer Configuration and User Configuration,
Administrative Templates, Torno.

**Intune.** In the Intune admin center choose Devices, Manage devices, Configuration, the
Import ADMX tab, Import, and upload the two files. Then choose Create, New policy, the platform
Windows 10 and later and the profile type Templates, Imported Administrative templates, and set
the policies. Intune takes one language file for a template, and only the English one. A later
version of the template has the same namespace, and Intune refuses a second upload of it: delete
the profiles that use the imported template and the template itself, then import the new files.
([Microsoft's instructions](https://learn.microsoft.com/mem/intune/configuration/administrative-templates-import-custom).)

**Without the templates.** Any tool that writes the registry values above gives the same
result, for example:

```
reg add HKLM\Software\Policies\Torno /v CheckForUpdates /t REG_DWORD /d 0 /f
reg add HKLM\Software\Policies\Torno /v OrganizationName /t REG_SZ /d "Example Corp" /f
```

## Check that a policy applies

While any policy is set, the application bar shows a briefcase and **Managed by** with the
organization's name (**Managed by your organization** without `OrganizationName`). A narrow
window shows **Managed** or the briefcase alone, and the tooltip has the whole text. Clicking it
opens Preferences, General. Preferences and About show the same line.

Open Preferences, General, Updates. With `CheckForUpdates` set, **Check at start and once a day**
is greyed out, shows the organization's choice, and says **Set by your organization**. With the
value `0`, **Check now** is unavailable too.

An agent or a script sees the same through the control channel: the reply of `ui.prefs` has a
`policy` object that lists the locked settings and the organization's name, and a request that changes a locked setting is
refused ([control protocol](https://github.com/tangency-labs/solidcraft/blob/main/docs/control-protocol.md)).

## A policy is not a launcher

[Managed by a launcher](updates.md#managed-by-a-launcher) is about a program that installs and
updates Torno for one person. A policy is an organization's setting. The two do not affect
each other.
