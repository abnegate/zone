import { expect, test } from "./fixtures";
import { mockCommonEndpoints, setupAuth } from "./helpers/auth";
import { blockServiceWorker, routeApi } from "./test-utils";

const SCREENSHOT = process.env.CHAT_HISTORY_SCREENSHOT;

const message = (
  id: string,
  chatId: string,
  role: "user" | "assistant",
  content: string,
) => ({
  id,
  chat_id: chatId,
  role,
  content,
  created_at: new Date().toISOString(),
});

const chatOf = (
  id: string,
  title: string,
  messages: ReturnType<typeof message>[],
) => ({
  id,
  title,
  model_name: "llama3.2",
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived: false,
  agent_enabled: false,
  messages,
});

test("switching back to a chat draws its history at once and reconciles behind it", async ({
  context,
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await blockServiceWorker(context);
  await mockCommonEndpoints(page);
  await setupAuth(page);

  const garden = chatOf("chat-garden", "Garden plans", [
    message("u1", "chat-garden", "user", "Where should the tomatoes go?"),
    message("a1", "chat-garden", "assistant", "Put them in the sunniest bed."),
  ]);
  const trip = chatOf("chat-trip", "Trip ideas", [
    message("u2", "chat-trip", "user", "Somewhere warm in March?"),
  ]);
  let gardenFetches = 0;
  let releaseGarden: () => void = () => {};

  await routeApi(page, /\/api\/chats(\/[^/?]+)?($|\?)/i, async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/chat-garden")) {
      gardenFetches += 1;
      if (gardenFetches > 1) {
        await new Promise<void>((resolve) => {
          releaseGarden = resolve;
        });
        await route.fulfill({
          json: {
            chat: {
              ...garden,
              messages: [
                ...garden.messages,
                message(
                  "a2",
                  "chat-garden",
                  "assistant",
                  "Water them every morning.",
                ),
              ],
            },
          },
        });
        return;
      }
      await route.fulfill({ json: { chat: garden } });
      return;
    }
    if (path.endsWith("/chat-trip")) {
      await route.fulfill({ json: { chat: trip } });
      return;
    }
    await route.fulfill({ json: { chats: [garden, trip] } });
  });
  await page.routeWebSocket(/\/ws\/chats\//, () => {});

  await page.goto("/chats");
  await page.getByText("Garden plans", { exact: true }).click();
  await expect(page.getByText("Put them in the sunniest bed.")).toBeVisible();

  await page.getByText("Trip ideas", { exact: true }).click();
  await expect(page.getByText("Somewhere warm in March?")).toBeVisible();

  await page.getByText("Garden plans", { exact: true }).click();
  await expect(page.getByText("Put them in the sunniest bed.")).toBeVisible();
  await expect(page.locator(".chat-placeholder .chats-loading")).toHaveCount(0);
  await expect.poll(() => gardenFetches).toBe(2);
  if (SCREENSHOT) await page.screenshot({ path: `${SCREENSHOT}-revisit.png` });

  releaseGarden();
  await expect(page.getByText("Water them every morning.")).toBeVisible();
  if (SCREENSHOT)
    await page.screenshot({ path: `${SCREENSHOT}-reconciled.png` });
});
