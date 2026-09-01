package blinded;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.Set;








public class BxTempFileRaceBy {

    public void store(String data) throws IOException {
        Set<PosixFilePermission> perms = PosixFilePermissions.fromString("rw-------");
        /*ANCHOR_1*/
        Path tmp = Files.createTempFile("report-", ".tmp",
                PosixFilePermissions.asFileAttribute(perms));
        Files.write(tmp, data.getBytes());
    }

    public static void main(String[] args) throws IOException {
        new BxTempFileRaceBy().store("localhost-demo-data");
    }
}
