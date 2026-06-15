"""
mTLS org identity extraction and OrgTable authorization.
In LOCAL_MODE the org_id comes from the X-Test-Org-Id header.
"""
import os
import re
from .dynamodb import get_item
from .response import forbidden


class AuthError(Exception):
    """Carries a ready API Gateway response dict."""
    def __init__(self, response: dict):
        self.response = response


def get_authenticated_org(event: dict) -> str:
    if os.environ.get("LOCAL_MODE") == "true":
        headers = event.get("headers") or {}
        org_id = headers.get("x-test-org-id") or headers.get("X-Test-Org-Id", "")
        if not org_id:
            raise AuthError(forbidden("Missing X-Test-Org-Id header in LOCAL_MODE"))
    else:
        try:
            subject_dn = (
                event["requestContext"]["authentication"]["clientCert"]["subjectDN"]
            )
            org_id = _extract_cn(subject_dn)
        except (KeyError, TypeError):
            raise AuthError(forbidden("Missing or invalid mTLS client certificate"))

    if not org_id:
        raise AuthError(forbidden("Could not determine org_id"))

    item = get_item("ORG_TABLE", {"org_id": org_id})
    if not item:
        raise AuthError(forbidden(f"org_id '{org_id}' is not registered"))
    if item.get("status") != "ACTIVE":
        raise AuthError(forbidden(f"org_id '{org_id}' is suspended"))

    return org_id


def _extract_cn(subject_dn: str) -> str:
    match = re.search(r"(?:^|,)\s*CN\s*=\s*([^,]+)", subject_dn, re.IGNORECASE)
    return match.group(1).strip() if match else ""
