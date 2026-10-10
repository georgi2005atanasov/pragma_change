import { test, expect } from "@playwright/test";

test("remote operations, collection, and assisted setup are reviewable", async ({
  page,
}) => {
  const mutations: { path: string; body: any }[] = [];
  await page.route("**/api/**", async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname.slice(5);
    if (request.method() === "POST")
      mutations.push({ path, body: request.postDataJSON() });
    const data =
      path === "overview"
        ? {}
        : path === "onboarding/provider"
          ? null
          : path === "servers"
            ? [
                {
                  id: "server-1",
                  name: "Proxy VM",
                  address: "proxy.internal",
                  port: 22,
                  username: "deploy",
                  discovery: {
                    fingerprint: "abc",
                    locations: [
                      {
                        id: "location-1",
                        path: "/etc/nginx/site.conf",
                        description: "location /",
                      },
                    ],
                    modules: [],
                    observed: null,
                  },
                },
              ]
            : path === "onboarding/jobs"
              ? [
                  {
                    id: "draft-1",
                    status: "review",
                    draft: {
                      summary: "Prepare export detection",
                      assumptions: ["Actor comes from verified authentication"],
                      action_mappings: [],
                      pipelines: [],
                      behavior_policy: null,
                    },
                    report: { validation: "passed" },
                  },
                ]
              : [];
    await route.fulfill({
      json: request.method() === "POST" ? { id: "job-1" } : data,
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Servers", exact: true }).click();
  await expect(
    page.getByText("Register an Nginx server", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Private SSH key", { exact: true })).toHaveValue(
    "",
  );
  await page
    .getByRole("combobox", { name: "Server", exact: true })
    .selectOption("server-1");
  await page.getByRole("button", { name: /inspect installation/ }).click();
  await expect
    .poll(() =>
      mutations.some(
        (m) =>
          m.path === "servers/server-1/jobs" && m.body.operation === "inspect",
      ),
    )
    .toBe(true);
  await page.getByRole("button", { name: "Behavior", exact: true }).click();
  await expect(
    page.getByText("Connect an application", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Assisted setup", exact: true })
    .click();
  await expect(
    page.getByText("Claude connection", { exact: true }),
  ).toBeVisible();
  await expect(
    page
      .getByText("Actor comes from verified authentication", { exact: false })
      .first(),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Apply reviewed setup" }),
  ).toBeDisabled();
  await page
    .getByLabel(
      "I reviewed the identity mappings, prerequisites, and assumptions.",
    )
    .check();
  await expect(
    page.getByRole("button", { name: "Apply reviewed setup" }),
  ).toBeEnabled();
  await page.screenshot({
    path: "../../build/screenshots/expansion-assisted.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
});
