/// Unit tests for hostMatchesSubmission (pure function, no I/O).
/// Run: node --test validate.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { hostMatchesSubmission, validateRecord } from "./src/validate.ts";

test("exact host matches", () => {
  assert.equal(hostMatchesSubmission("cal.com", "cal.com"), true);
});

test("www prefix and case are ignored on both sides", () => {
  assert.equal(hostMatchesSubmission("www.cal.com", "cal.com"), true);
  assert.equal(hostMatchesSubmission("cal.com", "www.cal.com"), true);
  assert.equal(hostMatchesSubmission("CAL.com", "cal.com"), true);
});

test("a valid subdomain of the submitted host matches", () => {
  assert.equal(hostMatchesSubmission("docs.cursor.com", "cursor.com"), true);
  assert.equal(hostMatchesSubmission("a.b.example.com", "example.com"), true);
});

test("a sibling or parent host does not match", () => {
  assert.equal(hostMatchesSubmission("cursor.com.evil.com", "cursor.com"), false);
  assert.equal(hostMatchesSubmission("notcursor.com", "cursor.com"), false);
  assert.equal(hostMatchesSubmission("cursor.com", "docs.cursor.com"), false);
});

test("multi-tenant suffixes only match exactly", () => {
  assert.equal(hostMatchesSubmission("foo.github.io", "foo.github.io"), true);
  assert.equal(hostMatchesSubmission("other.github.io", "foo.github.io"), false);
  assert.equal(hostMatchesSubmission("foo.github.io", "github.io"), false);
  assert.equal(hostMatchesSubmission("example.co.uk", "example.co.uk"), true);
  assert.equal(hostMatchesSubmission("sub.co.uk", "co.uk"), false);
  assert.equal(hostMatchesSubmission("a.vercel.app", "b.vercel.app"), false);
});

test("empty hosts never match", () => {
  assert.equal(hostMatchesSubmission("", "cal.com"), false);
  assert.equal(hostMatchesSubmission("cal.com", ""), false);
});

test("a non-string host is rejected without coercion", () => {
  const base = {
    host: "example.com",
    status: "verified",
    checked_at: new Date().toISOString(),
    pages_checked: ["https://example.com/"],
    tools: [],
  };
  assert.equal(validateRecord(base, "example.com").ok, true);
  // ["example.com"] would String()-coerce to a matching host; it must fail.
  assert.equal(validateRecord({ ...base, host: ["example.com"] }, "example.com").ok, false);
  assert.equal(validateRecord({ ...base, host: 42 }, "example.com").ok, false);
});
