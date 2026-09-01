
package blinded;








public class FilePermissionGateway {

    
    public String apply(String name, String mode) {
        // 语义等价：Set<PosixFilePermission> perms = parseOctal(mode); Files.setPosixFilePermissions(path, perms)
        /*ANCHOR_1*/
        System.out.println("[fs-chmod] chmod " + mode + " " + name);
        return "stored:" + name + ":" + mode;
    }
}
