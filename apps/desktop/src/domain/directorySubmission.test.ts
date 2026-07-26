import { describe, expect, it } from "vitest";

import { resolveDirectorySubmission } from "./directorySubmission";

describe("resolveDirectorySubmission", () => {
  it("treats ordinary words as a directory search", () => {
    expect(resolveDirectorySubmission("open source rooms")).toEqual({
      kind: "search",
      term: "open source rooms"
    });
  });

  it("joins a pasted matrix.to room link, carrying its via servers", () => {
    expect(
      resolveDirectorySubmission(
        "https://matrix.to/#/%23room%3Aexample.invalid?via=first.invalid&via=second.invalid"
      )
    ).toEqual({
      kind: "join",
      roomIdOrAlias: "#room:example.invalid",
      viaServers: ["first.invalid", "second.invalid"]
    });
  });

  it("joins a pasted room id link, where via servers are the only routing hint", () => {
    expect(
      resolveDirectorySubmission("https://matrix.to/#/%21room%3Aexample.invalid?via=only.invalid")
    ).toEqual({
      kind: "join",
      roomIdOrAlias: "!room:example.invalid",
      viaServers: ["only.invalid"]
    });
  });

  it("joins a typed alias, because the sigil already names one room", () => {
    expect(resolveDirectorySubmission("  #room:example.invalid  ")).toEqual({
      kind: "join",
      roomIdOrAlias: "#room:example.invalid",
      viaServers: []
    });
  });

  it("joins a typed room id", () => {
    expect(resolveDirectorySubmission("!room:example.invalid")).toEqual({
      kind: "join",
      roomIdOrAlias: "!room:example.invalid",
      viaServers: []
    });
  });

  it("searches for a sigil without a server, which names no single room", () => {
    expect(resolveDirectorySubmission("#room")).toEqual({ kind: "search", term: "#room" });
  });

  it("reports a user link as unsupported rather than joining a person", () => {
    expect(
      resolveDirectorySubmission("https://matrix.to/#/%40someone%3Aexample.invalid")
    ).toEqual({ kind: "user", userId: "@someone:example.invalid" });
  });

  it("treats blank input as nothing to do", () => {
    expect(resolveDirectorySubmission("   ")).toEqual({ kind: "empty" });
  });
});

describe("pasted user ids", () => {
  it("a bare MXID names a person, not a search term", () => {
    // Classifying it as a search returned "no public rooms found", which reads
    // as if the person did not exist (#330).
    expect(resolveDirectorySubmission("@someone:example.invalid")).toEqual({
      kind: "user",
      userId: "@someone:example.invalid"
    });
  });

  it("an @ without a server part is still just text to search for", () => {
    expect(resolveDirectorySubmission("@someone")).toEqual({
      kind: "search",
      term: "@someone"
    });
  });
});
