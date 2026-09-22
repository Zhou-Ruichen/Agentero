import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FsDirEntry } from "@/lib/core/bindings";

// A remote (SSH) vault tree is built renderer-side through `remoteList`, so
// the sync-store hiding rule lives in `lib/vault/tree.ts`. Fake the session
// with an in-memory directory map keyed by vault-relative path.
const { remoteDirs } = vi.hoisted(() => ({
	remoteDirs: new Map<string, FsDirEntry[]>(),
}));

vi.mock("@/lib/vault/remote/remote-vault", () => ({
	isRemoteVaultHandle: (path: string | null | undefined) =>
		!!path && path.startsWith("remote:"),
	remoteSessionIdFromHandle: (handle: string) =>
		handle.slice("remote:".length).trim() || null,
	remoteList: vi.fn(
		async (_sessionId: string, path = "") => remoteDirs.get(path) ?? [],
	),
}));

import { listVaultDirChildren, loadVaultTree } from "@/lib/vault/tree";

const entry = (name: string, path: string, isDir: boolean): FsDirEntry => ({
	name,
	path,
	isDir,
	isFile: !isDir,
});

const names = (nodes: { name: string }[]) => nodes.map((n) => n.name);

beforeEach(() => {
	remoteDirs.clear();
	// A sync store mirrored into the vault root by a desktop WebDAV client,
	// plus nested same-named user content that must stay visible.
	remoteDirs.set("", [
		entry("blobs", "blobs", true),
		entry("manifests", "manifests", true),
		entry("HEAD", "HEAD", false),
		entry("vault.json", "vault.json", false),
		entry("notes", "notes", true),
	]);
	remoteDirs.set("notes", [
		entry("blobs", "notes/blobs", true),
		entry("HEAD", "notes/HEAD", false),
		entry("idea.md", "notes/idea.md", false),
	]);
	remoteDirs.set("notes/blobs", [
		entry("keep.md", "notes/blobs/keep.md", false),
	]);
});

describe("remote vault tree hides mirrored sync-store artifacts at the root", () => {
	it("loadVaultTree filters root-level store entries only", async () => {
		const tree = await loadVaultTree("remote:s1");
		expect(names(tree)).toEqual(["notes"]);

		const notes = tree[0];
		expect(names(notes.children ?? [])).toContain("blobs");
		expect(names(notes.children ?? [])).toContain("HEAD");
		expect(names(notes.children ?? [])).toContain("idea.md");
	});

	it("a root refresh via listVaultDirChildren applies the same filter", async () => {
		const children = await listVaultDirChildren("remote:s1", "remote:s1");
		expect(names(children)).toEqual(["notes"]);
	});

	it("expanding a nested folder still lists same-named entries", async () => {
		const children = await listVaultDirChildren("remote:s1", "remote:s1/notes");
		expect(names(children).sort()).toEqual(["HEAD", "blobs", "idea.md"]);
	});
});
