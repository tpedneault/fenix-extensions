//! `database.sample`: a small SQLite database to try the browser on --
//! a shop's customers, products and orders -- made afresh under `/data`
//! each time, so it needs no server, no file of your own and nothing
//! installed. The tutorial's lesson practises on it.

/// Where it's made: the extension's own folder.
pub const PATH: &str = "/data/sample.db";
/// The connection it is in the browser.
pub const NAME: &str = "sample.db";

/// What makes it.
pub const SCRIPT: &str = "
create table customers (id integer primary key, name text not null, email text unique, city text, joined text not null);
create table products (id integer primary key, name text not null, price real not null, stock integer not null default 0);
create table orders (id integer primary key, customer_id integer not null references customers(id), placed text not null, status text not null);
create table order_items (order_id integer not null references orders(id), product_id integer not null references products(id), quantity integer not null, primary key (order_id, product_id));
create index orders_customer on orders(customer_id);
insert into customers values
  (1, 'Ada Lovelace', 'ada@example.com', 'London', '2024-01-05'),
  (2, 'Grace Hopper', 'grace@example.com', 'New York', '2024-01-19'),
  (3, 'Katherine Johnson', NULL, 'Hampton', '2024-02-02'),
  (4, 'Alan Turing', 'alan@example.com', 'Manchester', '2024-02-14'),
  (5, 'Margaret Hamilton', 'margaret@example.com', 'Boston', '2024-03-01'),
  (6, 'Edsger Dijkstra', NULL, 'Austin', '2024-03-11');
insert into products values
  (1, 'Notebook', 4.5, 120),
  (2, 'Fountain pen', 32.0, 14),
  (3, 'Ink, blue', 7.25, 60),
  (4, 'Desk lamp', 48.9, 0),
  (5, 'Slide rule', 19.99, 7);
insert into orders values
  (1, 1, '2024-03-02 09:14', 'shipped'),
  (2, 2, '2024-03-02 11:40', 'shipped'),
  (3, 1, '2024-03-05 16:03', 'delivered'),
  (4, 4, '2024-03-07 08:55', 'cancelled'),
  (5, 5, '2024-03-09 13:27', 'paid'),
  (6, 3, '2024-03-10 10:02', 'paid'),
  (7, 2, '2024-03-12 17:45', 'new');
insert into order_items values
  (1, 1, 3), (1, 3, 2), (2, 2, 1), (3, 5, 1), (3, 1, 1),
  (4, 4, 1), (5, 2, 2), (5, 3, 4), (6, 1, 10), (7, 5, 2);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lite::{create, Lite};
    use crate::result::Outcome;

    #[test]
    fn the_sample_is_made_and_reads_as_the_browser_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join(NAME).to_string_lossy().into_owned();
        create(&path, SCRIPT).unwrap();
        let mut db = Lite::open(&path, false).unwrap();
        let schema = db.schema().unwrap();
        let names: Vec<&str> = schema.namespaces[0].tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["customers", "order_items", "orders", "products"]);
        assert_eq!(schema.table("main", "order_items").unwrap().primary_key(), ["order_id", "product_id"]);
        assert_eq!(schema.table("main", "orders").unwrap().indexes.len(), 1);
        let out = db.run("select c.name, sum(i.quantity * p.price) as total from orders o join customers c on c.id = o.customer_id join order_items i on i.order_id = o.id join products p on p.id = i.product_id group by c.name order by total desc", 10);
        let [Outcome::Rows(r)] = &out[..] else { panic!("{out:?}") };
        assert_eq!(r.rows.len(), 5, "everyone who ordered");
        assert_eq!(r.rows[0][0].as_deref(), Some("Margaret Hamilton"), "{:?}", r.rows);
        let out = db.run("select count(*) from customers where email is null", 10);
        assert!(matches!(&out[..], [Outcome::Rows(r)] if r.rows[0][0].as_deref() == Some("2")));
    }
}
