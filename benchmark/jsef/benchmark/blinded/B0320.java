package blinded;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.Set;









public class BxTempFileBy {

    public void writeSecret(String secret) throws IOException {
        Set<PosixFilePermission> perms = PosixFilePermissions.fromString("rw-------");
        /*ANCHOR_1*/
        Path tmp = Files.createTempFile("app-", ".tmp",
                PosixFilePermissions.asFileAttribute(perms));
        Files.write(tmp, secret.getBytes());
    }

    public static void main(String[] args) throws IOException {
        new BxTempFileBy().writeSecret("localhost-demo-secret");
    }
}
