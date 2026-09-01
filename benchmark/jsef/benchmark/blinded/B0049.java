
package blinded;




public class FilePermissionGatewayBy {

    public String apply(String name, String byMode) {
        // 语义等价：Files.setPosixFilePermissions(path, fromMode(byMode))
        System.out.println("[fs-chmod][by] chmod " + byMode + " " + name);
        return "stored:" + name + ":" + byMode;
    }
}
