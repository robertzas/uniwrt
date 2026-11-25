import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Column, Table } from "./Table";

interface Node {
  id: string;
  name: string;
  clients: number;
}

const columns: Column<Node>[] = [
  { id: "name", header: "Name", cell: (n) => n.name },
  { id: "clients", header: "Clients", cell: (n) => n.clients, align: "end" },
];
const nodes = (count: number): Node[] =>
  Array.from({ length: count }, (_, i) => ({ id: `n${i}`, name: `ap-${i}`, clients: i }));

describe("Table", () => {
  it("renders a header and a row per item", () => {
    render(() => <Table label="Devices" columns={columns} rows={nodes(3)} rowKey={(n) => n.id} />);
    const table = screen.getByRole("table", { name: "Devices" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((h) => h.textContent),
    ).toEqual(["Name", "Clients"]);
    expect(within(table).getAllByRole("row")).toHaveLength(4);
    expect(within(table).getAllByRole("columnheader")[1]).toHaveClass("text-right");
  });

  it("shows the empty state without rows", () => {
    render(() => (
      <Table label="Devices" columns={columns} rows={[]} rowKey={(n) => n.id} empty="No devices" />
    ));
    expect(screen.getByText("No devices")).toBeInTheDocument();
  });

  it("activates rows by click and by keyboard", async () => {
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    render(() => (
      <Table
        label="Devices"
        columns={columns}
        rows={nodes(2)}
        rowKey={(n) => n.id}
        onRowClick={onRowClick}
      />
    ));
    const [, first, second] = screen.getAllByRole("row");
    await user.click(first as HTMLElement);
    expect(onRowClick).toHaveBeenLastCalledWith(expect.objectContaining({ id: "n0" }));
    (second as HTMLElement).focus();
    await user.keyboard("{Enter}");
    expect(onRowClick).toHaveBeenLastCalledWith(expect.objectContaining({ id: "n1" }));
  });

  it("keeps rows in place when the data updates", () => {
    const [rows, setRows] = createSignal(nodes(2));
    render(() => <Table label="Devices" columns={columns} rows={rows()} rowKey={(n) => n.id} />);
    const before = screen.getAllByRole("row")[1];
    setRows((r) => [...r, { id: "n9", name: "ap-9", clients: 9 }]);
    return Promise.resolve().then(() => {
      expect(screen.getAllByRole("row")).toHaveLength(4);
      expect(screen.getAllByRole("row")[1]).toBe(before);
    });
  });

  it("sorts by a sortable column's header, each way", async () => {
    const user = userEvent.setup();
    const sortable: Column<Node>[] = [
      { ...columns[0], sort: (a, b) => a.name.localeCompare(b.name) } as Column<Node>,
      { ...columns[1], sort: (a, b) => a.clients - b.clients } as Column<Node>,
    ];
    const rows = [
      { id: "a", name: "ap-b", clients: 1 },
      { id: "b", name: "ap-a", clients: 3 },
      { id: "c", name: "ap-c", clients: 2 },
    ];
    render(() => (
      <Table
        label="Devices"
        columns={sortable}
        rows={rows}
        rowKey={(n) => n.id}
        sortBy={{ column: "clients", desc: true }}
      />
    ));
    const names = () =>
      screen
        .getAllByRole("row")
        .slice(1)
        .map((row) => row.firstElementChild?.textContent);
    const [name, clients] = screen.getAllByRole("columnheader");
    expect(clients).toHaveAttribute("aria-sort", "descending");
    expect(name).toHaveAttribute("aria-sort", "none");
    expect(names()).toEqual(["ap-a", "ap-c", "ap-b"]);
    await user.click(screen.getByRole("button", { name: "Name" }));
    expect(name).toHaveAttribute("aria-sort", "ascending");
    expect(names()).toEqual(["ap-a", "ap-b", "ap-c"]);
    await user.click(screen.getByRole("button", { name: "Name" }));
    expect(name).toHaveAttribute("aria-sort", "descending");
    expect(names()).toEqual(["ap-c", "ap-b", "ap-a"]);
  });

  it("lays each row out as a card at phone width (D-148)", () => {
    const placed: Column<Node>[] = [
      { id: "name", header: "Name", cell: (n) => n.name },
      { id: "status", header: "Status", cell: () => "Online", card: "aside" },
      { id: "clients", header: "Clients", cell: (n) => n.clients, align: "end" },
      { id: "cpu", header: "CPU", cell: () => "5 %", card: "hide" },
    ];
    render(() => <Table label="Devices" columns={placed} rows={nodes(1)} rowKey={(n) => n.id} />);
    const [name, status, clients, cpu] = screen.getAllByRole("cell");
    expect(name).toHaveClass("max-md:col-start-1", "max-md:row-start-1");
    expect(name).not.toHaveClass("max-md:col-span-2");
    expect(status).toHaveClass("max-md:col-start-2", "max-md:row-start-1");
    expect(clients).toHaveClass("max-md:col-span-2", "max-md:flex");
    expect(clients).toHaveAttribute("data-label", "Clients");
    expect(cpu).toHaveClass("max-md:hidden");
    // Nothing sorts, so the header row has nothing to show on a phone.
    expect(screen.getAllByRole("rowgroup")[0]).toHaveClass("max-md:sr-only");
  });

  it("titles the card with the named column, spanning the row without an aside", () => {
    const titled: Column<Node>[] = [
      {
        id: "clients",
        header: "Clients",
        cell: (n) => n.clients,
        sort: (a, b) => a.clients - b.clients,
      },
      { id: "name", header: "Name", cell: (n) => n.name, card: "title" },
    ];
    render(() => <Table label="Devices" columns={titled} rows={nodes(1)} rowKey={(n) => n.id} />);
    const [clients, name] = screen.getAllByRole("cell");
    expect(name).toHaveClass("max-md:row-start-1", "max-md:col-span-2");
    expect(clients).toHaveClass("max-md:flex");
    // The sortable column's header stays on a phone, as a sort control; the other is for readers.
    const [byClients, byName] = screen.getAllByRole("columnheader");
    expect(byClients).not.toHaveClass("max-md:sr-only");
    expect(byName).toHaveClass("max-md:sr-only");
    expect(screen.getAllByRole("rowgroup")[0]).toHaveClass("max-md:block");
  });

  describe("virtualized", () => {
    // jsdom has no layout: give the scroll container a 400 px viewport.
    beforeEach(() => {
      vi.spyOn(Element.prototype, "clientHeight", "get").mockReturnValue(400);
      vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(800);
    });
    afterEach(() => vi.restoreAllMocks());

    it("renders only the rows in view above the threshold", async () => {
      render(() => (
        <Table
          label="Clients"
          columns={columns}
          rows={nodes(1000)}
          rowKey={(n) => n.id}
          virtualizeAbove={200}
        />
      ));
      await waitFor(() => expect(screen.getAllByRole("row").length).toBeGreaterThan(1));
      const rendered = screen.getAllByRole("row").length;
      expect(rendered).toBeLessThan(100);
      expect(screen.getByText("ap-0")).toBeInTheDocument();
      expect(screen.queryByText("ap-999")).not.toBeInTheDocument();
    });
  });
});
