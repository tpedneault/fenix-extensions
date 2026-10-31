import type { AddressInfo } from "node:net";
import type { Server } from "node:http";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createApp } from "./app.js";

let server: Server;
let base: string;

beforeAll(() => {
  server = createApp().listen(0);
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
  server.close();
});

describe("the app", () => {
  it("says it's healthy", async () => {
    const res = await fetch(`${base}/health`);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });

  it("echoes what it's sent", async () => {
    const res = await fetch(`${base}/echo`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ a: 1 }) });
    expect(res.status).toBe(201);
    expect(await res.json()).toEqual({ a: 1 });
  });
});
