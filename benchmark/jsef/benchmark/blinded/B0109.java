
package blinded;

import javax.servlet.RequestDispatcher;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.util.Arrays;
import java.util.List;


















public class UnvalidatedForward_By {

    private static final List<String> ALLOWED = Arrays.asList("/home", "/profile", "/dashboard");

    


    static void handle(HttpServletRequest req, HttpServletResponse resp) throws Exception {
        String userPath = req.getParameter("path");
        if (!ALLOWED.contains(userPath)) {
            resp.sendError(HttpServletResponse.SC_BAD_REQUEST, "invalid forward target");
            return;
        }
        RequestDispatcher dispatcher = req.getRequestDispatcher(userPath);
        /*ANCHOR_1*/
        dispatcher.forward(req, resp);
    }
}
