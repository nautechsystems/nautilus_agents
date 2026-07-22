#![cfg(feature = "testkit")]

use std::sync::Mutex;

use nautilus_agents::{
    client::{AgentClient, ClientFuture},
    protocol::{
        error::{ErrorCode, ProtocolError},
        identity::RequestId,
        live::LiveProposalRequest,
        receipt::{DecisionStatus, ProposalResponse},
    },
    testing,
};

struct MockClient {
    submitted: Mutex<Vec<LiveProposalRequest>>,
    requested_receipts: Mutex<Vec<RequestId>>,
    submit_response: ProposalResponse,
    receipt_response: ProposalResponse,
}

impl AgentClient for MockClient {
    fn submit<'a>(
        &'a self,
        request: &'a LiveProposalRequest,
    ) -> ClientFuture<'a, ProposalResponse> {
        self.submitted.lock().unwrap().push(request.clone());
        let response = self.submit_response.clone();
        Box::pin(async move { Ok(response) })
    }

    fn receipt<'a>(&'a self, request_id: &'a RequestId) -> ClientFuture<'a, ProposalResponse> {
        self.requested_receipts
            .lock()
            .unwrap()
            .push(request_id.clone());
        let response = self.receipt_response.clone();
        Box::pin(async move { Ok(response) })
    }
}

#[test]
fn test_mock_client_preserves_requests_and_public_responses() {
    let request = testing::reduce_position_request();
    let request_level = ProposalResponse::Error(ProtocolError {
        request_id: Some(request.request_id.clone()),
        code: ErrorCode::Forbidden,
        message: "principal cannot submit live proposals".to_owned(),
        retryable: false,
    });
    let decision_path = testing::rejected_response();
    let client = MockClient {
        submitted: Mutex::new(Vec::new()),
        requested_receipts: Mutex::new(Vec::new()),
        submit_response: request_level.clone(),
        receipt_response: decision_path.clone(),
    };

    let submit_actual = pollster::block_on(client.submit(&request)).unwrap();
    let receipt_actual = pollster::block_on(client.receipt(&request.request_id)).unwrap();

    assert_eq!(
        client.submitted.into_inner().unwrap(),
        vec![request.clone()]
    );
    assert_eq!(
        client.requested_receipts.into_inner().unwrap(),
        vec![request.request_id]
    );
    assert_eq!(submit_actual, request_level);
    assert_eq!(receipt_actual, decision_path);
    let ProposalResponse::Error(error) = submit_actual else {
        panic!("expected a request-level protocol response");
    };
    assert_eq!(error.code, ErrorCode::Forbidden);
    let ProposalResponse::Receipt(receipt) = receipt_actual else {
        panic!("expected a decision-path receipt response");
    };
    assert_eq!(receipt.status, DecisionStatus::Rejected);
    assert_eq!(receipt.error.unwrap().code, ErrorCode::Rejected);
}
