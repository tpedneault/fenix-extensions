package {{name_snake}};

public class App {
    public String greeting() {
        return "Hello from {{name}}";
    }

    public static void main(String[] args) {
        System.out.println(new App().greeting());
    }
}
