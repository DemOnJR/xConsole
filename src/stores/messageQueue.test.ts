import { describe, expect, it } from "vitest";
import {
  enqueueMessage,
  removeQueuedMessage,
  takeNextQueued,
  updateQueuedMessage,
} from "./messageQueue";

describe("message queue", () => {
  it("ignores blank enqueue", () => {
    expect(enqueueMessage([], "   ")).toEqual([]);
  });

  it("appends a trimmed follow-up", () => {
    const list = enqueueMessage([], "  check disk  ");
    expect(list).toHaveLength(1);
    expect(list[0].text).toBe("check disk");
    expect(list[0].id).toBeTruthy();
  });

  it("keeps attached images on a queued follow-up", () => {
    const list = enqueueMessage([], "look", [
      { media_type: "image/png", data: "AAAA", name: "shot.png" },
    ]);
    expect(list[0].images).toHaveLength(1);
    expect(list[0].images?.[0].name).toBe("shot.png");
  });

  it("lets the user edit a queued item before send", () => {
    const list = enqueueMessage([], "old");
    const edited = updateQueuedMessage(list, list[0].id, "new wording");
    expect(edited[0].text).toBe("new wording");
    expect(edited[0].id).toBe(list[0].id);
  });

  it("removes a queued item", () => {
    let list = enqueueMessage([], "a");
    list = enqueueMessage(list, "b");
    list = removeQueuedMessage(list, list[0].id);
    expect(list.map((i) => i.text)).toEqual(["b"]);
  });

  it("takeNext pops the first non-empty item", () => {
    let list = enqueueMessage([], "first");
    list = enqueueMessage(list, "second");
    const { next, rest } = takeNextQueued(list);
    expect(next?.text).toBe("first");
    expect(rest.map((i) => i.text)).toEqual(["second"]);
  });

  it("takeNext skips items the user emptied while editing", () => {
    let list = enqueueMessage([], "keep");
    list = updateQueuedMessage(list, list[0].id, "   ");
    list = enqueueMessage(list, "next");
    const { next, rest } = takeNextQueued(list);
    expect(next?.text).toBe("next");
    expect(rest).toEqual([]);
  });
});
