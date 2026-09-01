
package blinded;

import javax.servlet.RequestDispatcher;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;



















public class UnvalidatedForward {

    


    static void handle(HttpServletRequest req, HttpServletResponse resp) throws Exception {
        String userPath = req.getParameter("path");
        RequestDispatcher dispatcher = req.getRequestDispatcher(userPath);
        /*ANCHOR_1*/
        dispatcher.forward(req, resp);
    }
}
