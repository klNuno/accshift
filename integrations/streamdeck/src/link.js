// The switch link the running accshift GUI answers. Its parser is
// `parse_deep_link` in crates/accshift-app/src/lifecycle/deep_link/link.rs:
// `accshift://switch/<platformId>/<accountRef>`, three non-empty path
// segments, each percent-decoded, the platform matched lowercase and the
// account found by id first (`find_account`).

export const SCHEME = "accshift";

/** The longest link the app parses, in UTF-16 units (MAX_DEEP_LINK_URL_LENGTH). */
export const MAX_LINK_LENGTH = 2048;

/**
 * The link that switches `platformId` to the account whose id is `accountId`.
 * @param {string} platformId
 * @param {string} accountId
 * @returns {string}
 */
export function switchLink(platformId, accountId) {
  const platform = String(platformId ?? "")
    .trim()
    .toLowerCase();
  const account = String(accountId ?? "").trim();
  if (!platform || !account) {
    throw new TypeError("a switch link needs a platform and an account");
  }
  const link = `${SCHEME}://switch/${encodeURIComponent(platform)}/${encodeURIComponent(account)}`;
  if (link.length > MAX_LINK_LENGTH) {
    throw new RangeError(`switch link longer than ${MAX_LINK_LENGTH} characters`);
  }
  return link;
}
