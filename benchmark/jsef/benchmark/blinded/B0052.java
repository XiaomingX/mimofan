
package blinded;





public class RequestContextBy {

    private static final ThreadLocal<String> PRINCIPAL = new ThreadLocal<>();

    public void setPrincipal(String name) {
        PRINCIPAL.set(name);
    }

    public String getPrincipal() {
        return PRINCIPAL.get();
    }
}
