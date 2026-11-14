import { expect, test } from "bun:test";
import { createCountdownSaver } from "./countdownSaver";

test("leaving immediately saves the last countdown once", async () => {
  const saved: number[] = [];
  const saver = createCountdownSaver((value) => saved.push(value), 20);
  saver.queue(40);
  saver.queue(90);
  saver.flush();
  expect(saved).toEqual([90]);
  await new Promise((resolve) => setTimeout(resolve, 40));
  expect(saved).toEqual([90]);
});
