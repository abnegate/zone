#!/usr/bin/env python3
"""Provision two real tenants through the real API and write the state file the
live Playwright suite logs in with.

Tenant A is the one every feature is driven in. Tenant B exists to prove the
cross-tenant refusals from PR #46 from an attacker's side, with a genuinely
registered second account.
"""

import json
import os
import sys
import urllib.error
import urllib.request

API = os.environ.get('ZONE_API', 'http://127.0.0.1:8010')
OUT = os.environ.get('ZONE_LIVE_STATE', 'zone-live-state.json')


def call(method, path, body=None, token=None, expect=None):
    request = urllib.request.Request(f'{API}{path}', method=method)
    request.add_header('Content-Type', 'application/json')
    if token:
        request.add_header('Authorization', f'Bearer {token}')
    data = json.dumps(body).encode() if body is not None else None
    try:
        with urllib.request.urlopen(request, data, timeout=60) as response:
            payload = response.read()
            status = response.status
    except urllib.error.HTTPError as error:
        payload = error.read()
        status = error.code
    try:
        parsed = json.loads(payload) if payload else {}
    except ValueError:
        parsed = {'_raw': payload.decode('utf-8', 'replace')}
    if expect is not None and status not in expect:
        raise SystemExit(f'{method} {path} -> {status}: {json.dumps(parsed)[:600]}')
    return status, parsed


def account(email, password, name):
    status, payload = call(
        'POST',
        '/api/auth/register',
        {'email': email, 'password': password, 'display_name': name},
    )
    if status == 409:
        status, payload = call(
            'POST', '/api/auth/login', {'email': email, 'password': password}, expect=[200]
        )
    elif status != 201:
        raise SystemExit(f'register {email} -> {status}: {json.dumps(payload)[:400]}')
    return {
        'email': email,
        'password': password,
        'access_token': payload['access_token'],
        'refresh_token': payload['refresh_token'],
        'user': payload['user'],
        'permissions': payload.get('permissions', []),
    }


def tenant(who, org_name, slug, workspace_name):
    token = who['access_token']
    status, orgs = call('GET', '/api/organizations', token=token, expect=[200])
    existing = next(
        (org for org in orgs.get('organizations', []) if org['slug'] == slug), None
    )
    if existing:
        org = existing
    else:
        _, org = call(
            'POST',
            '/api/organizations',
            {'name': org_name, 'slug': slug, 'description': 'live verification tenant'},
            token=token,
            expect=[200, 201],
        )
        org = org.get('organization', org)
    _, spaces = call(
        'GET', f'/api/organizations/{org["id"]}/workspaces', token=token, expect=[200]
    )
    rows = spaces.get('workspaces', [])
    # The list is ordered by creation, newest first, so position picks whichever
    # workspace was made last rather than the one this rig seeded.
    workspace = next((row for row in rows if row.get('slug') == 'live'), None)
    if workspace is None:
        _, workspace = call(
            'POST',
            f'/api/organizations/{org["id"]}/workspaces',
            {'name': workspace_name, 'slug': 'live', 'description': 'live verification'},
            token=token,
            expect=[200, 201],
        )
        workspace = workspace.get('workspace', workspace)
    return org, workspace


def main():
    status, health = call('GET', '/health')
    if status != 200:
        raise SystemExit(f'server not healthy: {status}')

    owner = account('owner@zone.test', 'Verify-2026-Owner!', 'Owner One')
    intruder = account('intruder@zone.test', 'Verify-2026-Intruder!', 'Intruder Two')

    org_a, workspace_a = tenant(owner, 'Zone Verify', 'zone-verify', 'Verify Workspace')
    org_b, workspace_b = tenant(intruder, 'Other Tenant', 'other-tenant', 'Other Workspace')

    state = {
        'api': API,
        'owner': {**owner, 'organization': org_a, 'workspace': workspace_a},
        'intruder': {**intruder, 'organization': org_b, 'workspace': workspace_b},
    }
    with open(OUT, 'w') as handle:
        json.dump(state, handle, indent=2)
    print(
        json.dumps(
            {
                'owner_org': org_a['id'],
                'owner_workspace': workspace_a['id'],
                'intruder_org': org_b['id'],
                'intruder_workspace': workspace_b['id'],
                'state': os.path.abspath(OUT),
            },
            indent=2,
        )
    )


if __name__ == '__main__':
    sys.exit(main())
