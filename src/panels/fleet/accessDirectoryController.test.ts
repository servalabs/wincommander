import { describe, expect, test } from "bun:test";
import { createAccessDirectoryController } from "./accessDirectoryController";
import type { VaultAccessDirectory } from "./accessControlTypes";

const directory = (name = "Team"): VaultAccessDirectory => ({ schema_version: 1, users: [{ sid: "S-1-5-21-101", username: "Alice" }], groups: [{ id: "team", name, local_group: "WC_Team", member_sids: ["S-1-5-21-101"] }] });
const save = async (value: VaultAccessDirectory) => ({ directory: value, results: [] });
describe("protected machine group directory", () => {
  test("another admin reads the same machine store, and refresh sees remote edits", async () => {
    let durable = directory();
    const a = createAccessDirectoryController(async () => structuredClone(durable), save, () => {});
    const b = createAccessDirectoryController(async () => structuredClone(durable), save, () => {});
    await a.refresh(); await b.refresh();
    expect(a.getState().directory.groups).toEqual(b.getState().directory.groups);
    durable = directory("New name"); await b.refresh();
    expect(b.getState().directory.groups[0].name).toBe("New name");
  });
  test("automatic refresh preserves dirty drafts; explicit discard refreshes", async () => {
    const controller = createAccessDirectoryController(async () => directory(), save, () => {});
    await controller.refresh();
    controller.update(current => ({ ...current, groups: [{ ...current.groups[0], name: "Local draft" }] }));
    expect(await controller.refresh()).toBe(false);
    expect(controller.getState().directory.groups[0].name).toBe("Local draft");
    await controller.refresh(true);
    expect(controller.getState().directory.groups[0].name).toBe("Team");
  });
  test("account discovery does not mark group membership dirty", async () => {
    const controller = createAccessDirectoryController(async () => directory(), save, () => {});
    await controller.refresh();
    controller.update(current => ({ ...current, users: current.users.map(user => ({ ...user, isCurrent: true })) }));
    expect(controller.getState().dirty).toBe(false);
  });
  test("failed read preserves saved groups and exposes a retryable error", async () => {
    let fail = false;
    const controller = createAccessDirectoryController(async () => { if (fail) throw Error("offline"); return directory(); }, save, () => {});
    await controller.refresh(); fail = true; await controller.refresh();
    expect(controller.getState().directory.groups).toHaveLength(1);
    expect(controller.getState().error).toContain("could not be read");
    expect(controller.getState().loading).toBe(false);
  });
  test("accepted creator additions must read back before save is confirmed", async () => {
    let durable = directory();
    const controller = createAccessDirectoryController(async () => structuredClone(durable), async value => {
      durable = { ...value, groups: value.groups.map(group => group.id === "new" ? { ...group, member_sids: [...group.member_sids, "S-1-5-21-202"] } : group) };
      return { directory: structuredClone(durable), results: [] };
    }, () => {});
    await controller.refresh();
    controller.update(current => ({ ...current,
      users: [...current.users, { id: "sid:s-1-5-21-202", sid: "S-1-5-21-202", username: "Creator", isCurrent: true }],
      groups: [...current.groups, { id: "new", name: "New group", localGroup: "WC_New", userIds: [] }],
    }));
    const saved = await controller.save(controller.getState().directory);
    expect(saved.directory.groups[1].member_sids).toContain("S-1-5-21-202");
    expect(controller.getState().dirty).toBe(false);
  });
  test("mismatched readback and backend rejection never confirm save", async () => {
    const mismatch = createAccessDirectoryController(async () => directory(), async () => ({ directory: directory("Changed"), results: [] }), () => {});
    await mismatch.refresh();
    expect(await mismatch.save(mismatch.getState().directory).then(() => "unexpected success", error => error.message)).toBe("vault_group_readback_unconfirmed");
    const denied = createAccessDirectoryController(async () => directory(), async () => { throw Error("forbidden"); }, () => {});
    await denied.refresh();
    expect(await denied.save(denied.getState().directory).then(() => "unexpected success", error => error.message)).toBe("forbidden");
    expect(denied.getState().saving).toBe(false);
  });
  test("a slower refresh cannot overwrite edits made during its request", async () => {
    let resolve!: (value: VaultAccessDirectory) => void;
    const controller = createAccessDirectoryController(() => new Promise(done => { resolve = done; }), save, () => {});
    const pending = controller.refresh();
    controller.update(current => ({ ...current, groups: [{ id: "draft", name: "Draft", localGroup: "WC_Draft", userIds: [] }] }));
    resolve(directory()); await pending;
    expect(controller.getState().directory.groups[0].name).toBe("Draft");
  });
  test("refresh retains newly discovered accounts missing from the saved directory", async () => {
    const controller = createAccessDirectoryController(async () => directory(), save, () => {});
    await controller.refresh();
    controller.update(current => ({ ...current, users: [...current.users, { id: "sid:s-1-5-21-202", sid: "S-1-5-21-202", username: "Bob", isAvailable: true, isCurrent: true }] }));
    await controller.refresh();
    expect(controller.getState().directory.users.find(user => user.username === "Bob")?.isCurrent).toBe(true);
    expect(controller.getState().dirty).toBe(false);
  });
  test("another administrator's edit is detected before sending a stale save", async () => {
    let durable = directory(), writes = 0;
    const controller = createAccessDirectoryController(async () => durable, async value => { writes++; return save(value); }, () => {});
    await controller.refresh(); durable = directory("Other admin edit");
    expect(await controller.save(controller.getState().directory).then(() => "unexpected success", error => error.message)).toBe("vault_group_refresh_required");
    expect(writes).toBe(0);
  });
  test("silent no-op service replies cannot discard a changed draft", async () => {
    const controller = createAccessDirectoryController(async () => directory(), async () => save(directory()), () => {});
    await controller.refresh();
    controller.update(current => ({ ...current, groups: [{ ...current.groups[0], name: "Requested rename" }] }));
    expect(await controller.save(controller.getState().directory).then(() => "success", error => error.message)).toBe("vault_group_readback_unconfirmed");
    expect(controller.getState().dirty).toBe(true);
    expect(controller.getState().directory.groups[0].name).toBe("Requested rename");
  });
  test("partial acceptance cannot discard an omitted new group", async () => {
    const controller = createAccessDirectoryController(async () => directory(), async value => save({ ...value, groups: value.groups.slice(0, 1) }), () => {});
    await controller.refresh();
    controller.update(current => ({ ...current, groups: [...current.groups, { id: "new", name: "New", localGroup: "WC_New", userIds: [] }] }));
    expect(await controller.save(controller.getState().directory).then(() => "success", error => error.message)).toBe("vault_group_readback_unconfirmed");
    expect(controller.getState().dirty).toBe(true);
    expect(controller.getState().directory.groups).toHaveLength(2);
  });
  test("extra existing-group member is not creator normalization", async () => {
    const controller = createAccessDirectoryController(async () => directory(), async value => save({ ...value, groups: value.groups.map(group => ({ ...group, member_sids: [...group.member_sids, "S-1-5-21-202"] })) }), () => {});
    await controller.refresh();
    expect(await controller.save(controller.getState().directory).then(() => "success", error => error.message)).toBe("vault_group_readback_unconfirmed");
  });
  test("a genuinely unchanged save can confirm without modifying its group", async () => {
    const controller = createAccessDirectoryController(async () => directory(), save, () => {});
    await controller.refresh();
    await controller.save(controller.getState().directory);
    expect(controller.getState().dirty).toBe(false);
  });
});
