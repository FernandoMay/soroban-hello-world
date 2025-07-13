#![no_std]
use soroban_sdk::{contract, contractimpl, vec, Env, String, Vec};
// Savia Smart Contracts for Stellar Slingshot
// Based on ZkVM blockchain specification and Stellar Disbursement Platform

use zkvm::{
    contract::{Contract, PortableItem},
    constraint_system::ConstraintSystem,
    predicate::Predicate,
    transcript::Transcript,
    value::Value,
};
use bulletproofs::r1cs::R1CSProof;
use curve25519_dalek::{ristretto::RistrettoPoint, scalar::Scalar};
use std::collections::HashMap;

// ========== DATA STRUCTURES ==========

#[derive(Debug, Clone)]
pub struct Campaign {
    pub id: [u8; 32],
    pub title: String,
    pub description: String,
    pub beneficiary: RistrettoPoint,
    pub goal_amount: u64,
    pub current_amount: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub verified: bool,
    pub trust_score: u8,
    pub nft_count: u32,
    pub category: String,
    pub location: String,
    pub documents_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct Donation {
    pub id: [u8; 32],
    pub campaign_id: [u8; 32],
    pub donor: RistrettoPoint,
    pub amount: u64,
    pub timestamp: u64,
    pub nft_minted: bool,
    pub recurring: bool,
    pub anonymous: bool,
}

#[derive(Debug, Clone)]
pub struct TrustScore {
    pub entity: RistrettoPoint,
    pub score: u8,
    pub verification_level: u8,
    pub donation_count: u32,
    pub total_donated: u64,
    pub campaigns_created: u32,
    pub last_updated: u64,
}

#[derive(Debug, Clone)]
pub struct NFTBadge {
    pub id: [u8; 32],
    pub owner: RistrettoPoint,
    pub badge_type: String,
    pub campaign_id: Option<[u8; 32]>,
    pub minted_at: u64,
    pub metadata_uri: String,
}

// ========== CAMPAIGN MANAGEMENT CONTRACT ==========

pub struct CampaignContract {
    campaigns: HashMap<[u8; 32], Campaign>,
    platform_fee: u64, // Basis points (e.g., 200 = 2%)
    verification_threshold: u64,
}

impl CampaignContract {
    pub fn new(platform_fee: u64) -> Self {
        Self {
            campaigns: HashMap::new(),
            platform_fee,
            verification_threshold: 10000, // $100 in cents
        }
    }

    /// Create a new campaign
    pub fn create_campaign(
        &mut self,
        cs: &mut ConstraintSystem,
        title: String,
        description: String,
        beneficiary: RistrettoPoint,
        goal_amount: u64,
        duration_days: u64,
        category: String,
        location: String,
        documents_hash: [u8; 32],
    ) -> Result<[u8; 32], String> {
        // Generate campaign ID
        let mut transcript = Transcript::new(b"Savia.create_campaign");
        transcript.append_message(b"title", title.as_bytes());
        transcript.append_message(b"beneficiary", beneficiary.compress().as_bytes());
        transcript.append_u64(b"goal_amount", goal_amount);
        transcript.append_u64(b"timestamp", self.get_current_timestamp());
        
        let campaign_id = transcript.challenge_bytes(b"campaign_id");

        // Validate inputs
        if goal_amount == 0 {
            return Err("Goal amount must be greater than 0".to_string());
        }
        
        if duration_days == 0 || duration_days > 365 {
            return Err("Duration must be between 1 and 365 days".to_string());
        }

        let current_time = self.get_current_timestamp();
        let end_time = current_time + (duration_days * 24 * 60 * 60 * 1000); // Convert to milliseconds

        let campaign = Campaign {
            id: campaign_id,
            title,
            description,
            beneficiary,
            goal_amount,
            current_amount: 0,
            start_time: current_time,
            end_time,
            verified: false,
            trust_score: 0,
            nft_count: 0,
            category,
            location,
            documents_hash,
        };

        // Add ZK constraint for campaign creation
        let (_, _, campaign_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(campaign_commitment);

        self.campaigns.insert(campaign_id, campaign);
        Ok(campaign_id)
    }

    /// Verify a campaign (admin function)
    pub fn verify_campaign(
        &mut self,
        cs: &mut ConstraintSystem,
        campaign_id: [u8; 32],
        trust_score: u8,
    ) -> Result<(), String> {
        let campaign = self.campaigns.get_mut(&campaign_id)
            .ok_or("Campaign not found")?;

        campaign.verified = true;
        campaign.trust_score = trust_score;

        // ZK constraint for verification
        let (_, _, verification_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(verification_commitment);

        Ok(())
    }

    /// Get campaign details
    pub fn get_campaign(&self, campaign_id: [u8; 32]) -> Option<&Campaign> {
        self.campaigns.get(&campaign_id)
    }

    /// Update campaign progress
    pub fn update_campaign_progress(
        &mut self,
        campaign_id: [u8; 32],
        additional_amount: u64,
    ) -> Result<(), String> {
        let campaign = self.campaigns.get_mut(&campaign_id)
            .ok_or("Campaign not found")?;

        campaign.current_amount += additional_amount;
        Ok(())
    }

    fn get_current_timestamp(&self) -> u64 {
        // In real implementation, this would get blockchain timestamp
        1234567890000 // Placeholder
    }
}

// ========== DONATION CONTRACT ==========

pub struct DonationContract {
    donations: HashMap<[u8; 32], Donation>,
    campaign_contract: CampaignContract,
    trust_contract: TrustScoreContract,
    nft_contract: NFTContract,
}

impl DonationContract {
    pub fn new(
        campaign_contract: CampaignContract,
        trust_contract: TrustScoreContract,
        nft_contract: NFTContract,
    ) -> Self {
        Self {
            donations: HashMap::new(),
            campaign_contract,
            trust_contract,
            nft_contract,
        }
    }

    /// Process a donation
    pub fn donate(
        &mut self,
        cs: &mut ConstraintSystem,
        campaign_id: [u8; 32],
        donor: RistrettoPoint,
        amount: u64,
        anonymous: bool,
        mint_nft: bool,
    ) -> Result<[u8; 32], String> {
        // Validate campaign exists and is active
        let campaign = self.campaign_contract.get_campaign(campaign_id)
            .ok_or("Campaign not found")?;

        let current_time = self.get_current_timestamp();
        if current_time > campaign.end_time {
            return Err("Campaign has ended".to_string());
        }

        if amount == 0 {
            return Err("Donation amount must be greater than 0".to_string());
        }

        // Generate donation ID
        let mut transcript = Transcript::new(b"Savia.donate");
        transcript.append_message(b"campaign_id", &campaign_id);
        transcript.append_message(b"donor", donor.compress().as_bytes());
        transcript.append_u64(b"amount", amount);
        transcript.append_u64(b"timestamp", current_time);
        
        let donation_id = transcript.challenge_bytes(b"donation_id");

        // Calculate platform fee
        let platform_fee = (amount * self.campaign_contract.platform_fee) / 10000;
        let net_amount = amount - platform_fee;

        // Create donation record
        let donation = Donation {
            id: donation_id,
            campaign_id,
            donor,
            amount: net_amount,
            timestamp: current_time,
            nft_minted: mint_nft,
            recurring: false,
            anonymous,
        };

        // ZK constraints for donation validation
        let (amount_var, _, amount_commitment) = cs.allocate_multiplier(Some(Scalar::from(amount))).unwrap();
        let (fee_var, _, fee_commitment) = cs.allocate_multiplier(Some(Scalar::from(platform_fee))).unwrap();
        let (net_var, _, net_commitment) = cs.allocate_multiplier(Some(Scalar::from(net_amount))).unwrap();

        // Constraint: amount = fee + net_amount
        cs.constrain(amount_var - fee_var - net_var);

        // Update campaign progress
        self.campaign_contract.update_campaign_progress(campaign_id, net_amount)?;

        // Update trust score
        self.trust_contract.update_donor_score(donor, net_amount)?;

        // Mint NFT if requested
        if mint_nft {
            self.nft_contract.mint_donation_nft(
                cs,
                donor,
                campaign_id,
                donation_id,
                net_amount,
            )?;
        }

        self.donations.insert(donation_id, donation);
        Ok(donation_id)
    }

    /// Get donation details
    pub fn get_donation(&self, donation_id: [u8; 32]) -> Option<&Donation> {
        self.donations.get(&donation_id)
    }

    /// Get donations for a campaign
    pub fn get_campaign_donations(&self, campaign_id: [u8; 32]) -> Vec<&Donation> {
        self.donations.values()
            .filter(|d| d.campaign_id == campaign_id)
            .collect()
    }

    fn get_current_timestamp(&self) -> u64 {
        1234567890000 // Placeholder
    }
}

// ========== TRUST SCORE CONTRACT ==========

pub struct TrustScoreContract {
    trust_scores: HashMap<RistrettoPoint, TrustScore>,
}

impl TrustScoreContract {
    pub fn new() -> Self {
        Self {
            trust_scores: HashMap::new(),
        }
    }

    /// Initialize trust score for new user
    pub fn initialize_trust_score(&mut self, entity: RistrettoPoint) -> Result<(), String> {
        if self.trust_scores.contains_key(&entity) {
            return Err("Trust score already exists".to_string());
        }

        let trust_score = TrustScore {
            entity,
            score: 50, // Start with neutral score
            verification_level: 0,
            donation_count: 0,
            total_donated: 0,
            campaigns_created: 0,
            last_updated: self.get_current_timestamp(),
        };

        self.trust_scores.insert(entity, trust_score);
        Ok(())
    }

    /// Update donor trust score
    pub fn update_donor_score(&mut self, donor: RistrettoPoint, amount: u64) -> Result<(), String> {
        let trust_score = self.trust_scores.get_mut(&donor)
            .ok_or("Trust score not found")?;

        trust_score.donation_count += 1;
        trust_score.total_donated += amount;
        trust_score.last_updated = self.get_current_timestamp();

        // Calculate new score based on donation history
        let donation_factor = (trust_score.donation_count.min(100) as f64) / 100.0;
        let amount_factor = (trust_score.total_donated.min(100000) as f64) / 100000.0;
        let consistency_factor = if trust_score.donation_count > 1 { 1.2 } else { 1.0 };

        let new_score = 50.0 + (25.0 * donation_factor) + (20.0 * amount_factor) * consistency_factor;
        trust_score.score = new_score.min(100.0) as u8;

        Ok(())
    }

    /// Update campaign creator trust score
    pub fn update_creator_score(&mut self, creator: RistrettoPoint, successful: bool) -> Result<(), String> {
        let trust_score = self.trust_scores.get_mut(&creator)
            .ok_or("Trust score not found")?;

        trust_score.campaigns_created += 1;
        trust_score.last_updated = self.get_current_timestamp();

        if successful {
            trust_score.score = (trust_score.score + 5).min(100);
        } else {
            trust_score.score = trust_score.score.saturating_sub(10);
        }

        Ok(())
    }

    /// Get trust score
    pub fn get_trust_score(&self, entity: RistrettoPoint) -> Option<&TrustScore> {
        self.trust_scores.get(&entity)
    }

    fn get_current_timestamp(&self) -> u64 {
        1234567890000 // Placeholder
    }
}

// ========== NFT CONTRACT ==========

pub struct NFTContract {
    nfts: HashMap<[u8; 32], NFTBadge>,
    nft_counter: u64,
}

impl NFTContract {
    pub fn new() -> Self {
        Self {
            nfts: HashMap::new(),
            nft_counter: 0,
        }
    }

    /// Mint donation NFT
    pub fn mint_donation_nft(
        &mut self,
        cs: &mut ConstraintSystem,
        owner: RistrettoPoint,
        campaign_id: [u8; 32],
        donation_id: [u8; 32],
        amount: u64,
    ) -> Result<[u8; 32], String> {
        self.nft_counter += 1;

        // Generate NFT ID
        let mut transcript = Transcript::new(b"Savia.mint_nft");
        transcript.append_message(b"owner", owner.compress().as_bytes());
        transcript.append_message(b"campaign_id", &campaign_id);
        transcript.append_message(b"donation_id", &donation_id);
        transcript.append_u64(b"amount", amount);
        transcript.append_u64(b"counter", self.nft_counter);
        
        let nft_id = transcript.challenge_bytes(b"nft_id");

        // Determine badge type based on amount
        let badge_type = self.get_badge_type(amount);

        // Create metadata URI
        let metadata_uri = format!(
            "https://savia.org/nft/{}/{}",
            hex::encode(campaign_id),
            hex::encode(nft_id)
        );

        let nft_badge = NFTBadge {
            id: nft_id,
            owner,
            badge_type,
            campaign_id: Some(campaign_id),
            minted_at: self.get_current_timestamp(),
            metadata_uri,
        };

        // ZK constraint for NFT minting
        let (_, _, nft_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(nft_commitment);

        self.nfts.insert(nft_id, nft_badge);
        Ok(nft_id)
    }

    /// Mint achievement badge
    pub fn mint_achievement_badge(
        &mut self,
        cs: &mut ConstraintSystem,
        owner: RistrettoPoint,
        badge_type: String,
    ) -> Result<[u8; 32], String> {
        self.nft_counter += 1;

        let mut transcript = Transcript::new(b"Savia.mint_badge");
        transcript.append_message(b"owner", owner.compress().as_bytes());
        transcript.append_message(b"badge_type", badge_type.as_bytes());
        transcript.append_u64(b"counter", self.nft_counter);
        
        let nft_id = transcript.challenge_bytes(b"nft_id");

        let metadata_uri = format!(
            "https://savia.org/badge/{}/{}",
            badge_type,
            hex::encode(nft_id)
        );

        let nft_badge = NFTBadge {
            id: nft_id,
            owner,
            badge_type,
            campaign_id: None,
            minted_at: self.get_current_timestamp(),
            metadata_uri,
        };

        // ZK constraint for badge minting
        let (_, _, badge_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(badge_commitment);

        self.nfts.insert(nft_id, nft_badge);
        Ok(nft_id)
    }

    /// Get NFT details
    pub fn get_nft(&self, nft_id: [u8; 32]) -> Option<&NFTBadge> {
        self.nfts.get(&nft_id)
    }

    /// Get NFTs owned by user
    pub fn get_user_nfts(&self, owner: RistrettoPoint) -> Vec<&NFTBadge> {
        self.nfts.values()
            .filter(|nft| nft.owner == owner)
            .collect()
    }

    /// Burn NFT (for milestones or conversions)
    pub fn burn_nft(
        &mut self,
        cs: &mut ConstraintSystem,
        nft_id: [u8; 32],
        owner: RistrettoPoint,
    ) -> Result<(), String> {
        let nft = self.nfts.get(&nft_id)
            .ok_or("NFT not found")?;

        if nft.owner != owner {
            return Err("Not the owner of this NFT".to_string());
        }

        // ZK constraint for NFT burning
        let (_, _, burn_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(burn_commitment);

        self.nfts.remove(&nft_id);
        Ok(())
    }

    fn get_badge_type(&self, amount: u64) -> String {
        match amount {
            0..=999 => "Bronze Supporter".to_string(),
            1000..=4999 => "Silver Supporter".to_string(),
            5000..=9999 => "Gold Supporter".to_string(),
            10000..=49999 => "Platinum Supporter".to_string(),
            _ => "Diamond Supporter".to_string(),
        }
    }

    fn get_current_timestamp(&self) -> u64 {
        1234567890000 // Placeholder
    }
}

// ========== DISBURSEMENT CONTRACT ==========

pub struct DisbursementContract {
    disbursements: HashMap<[u8; 32], Disbursement>,
    campaign_contract: CampaignContract,
}

#[derive(Debug, Clone)]
pub struct Disbursement {
    pub id: [u8; 32],
    pub campaign_id: [u8; 32],
    pub recipient: RistrettoPoint,
    pub amount: u64,
    pub milestone: String,
    pub status: DisbursementStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
    pub proof_documents: Vec<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisbursementStatus {
    Pending,
    Approved,
    Executed,
    Rejected,
}

impl DisbursementContract {
    pub fn new(campaign_contract: CampaignContract) -> Self {
        Self {
            disbursements: HashMap::new(),
            campaign_contract,
        }
    }

    /// Create disbursement request
    pub fn create_disbursement(
        &mut self,
        cs: &mut ConstraintSystem,
        campaign_id: [u8; 32],
        recipient: RistrettoPoint,
        amount: u64,
        milestone: String,
        proof_documents: Vec<[u8; 32]>,
    ) -> Result<[u8; 32], String> {
        let campaign = self.campaign_contract.get_campaign(campaign_id)
            .ok_or("Campaign not found")?;

        if amount > campaign.current_amount {
            return Err("Disbursement amount exceeds available funds".to_string());
        }

        let mut transcript = Transcript::new(b"Savia.create_disbursement");
        transcript.append_message(b"campaign_id", &campaign_id);
        transcript.append_message(b"recipient", recipient.compress().as_bytes());
        transcript.append_u64(b"amount", amount);
        transcript.append_message(b"milestone", milestone.as_bytes());
        
        let disbursement_id = transcript.challenge_bytes(b"disbursement_id");

        let disbursement = Disbursement {
            id: disbursement_id,
            campaign_id,
            recipient,
            amount,
            milestone,
            status: DisbursementStatus::Pending,
            created_at: self.get_current_timestamp(),
            executed_at: None,
            proof_documents,
        };

        // ZK constraint for disbursement creation
        let (_, _, disbursement_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(disbursement_commitment);

        self.disbursements.insert(disbursement_id, disbursement);
        Ok(disbursement_id)
    }

    /// Execute approved disbursement
    pub fn execute_disbursement(
        &mut self,
        cs: &mut ConstraintSystem,
        disbursement_id: [u8; 32],
    ) -> Result<(), String> {
        let disbursement = self.disbursements.get_mut(&disbursement_id)
            .ok_or("Disbursement not found")?;

        if disbursement.status != DisbursementStatus::Approved {
            return Err("Disbursement not approved".to_string());
        }

        // ZK constraint for disbursement execution
        let (_, _, execution_commitment) = cs.allocate_multiplier(None).unwrap();
        cs.constrain(execution_commitment);

        disbursement.status = DisbursementStatus::Executed;
        disbursement.executed_at = Some(self.get_current_timestamp());

        Ok(())
    }

    fn get_current_timestamp(&self) -> u64 {
        1234567890000 // Placeholder
    }
}

// ========== MAIN CONTRACT ORCHESTRATOR ==========

pub struct SaviaProtocol {
    pub campaign_contract: CampaignContract,
    pub donation_contract: DonationContract,
    pub trust_contract: TrustScoreContract,
    pub nft_contract: NFTContract,
    pub disbursement_contract: DisbursementContract,
}

impl SaviaProtocol {
    pub fn new(platform_fee: u64) -> Self {
        let campaign_contract = CampaignContract::new(platform_fee);
        let trust_contract = TrustScoreContract::new();
        let nft_contract = NFTContract::new();
        let disbursement_contract = DisbursementContract::new(campaign_contract.clone());
        
        let donation_contract = DonationContract::new(
            campaign_contract.clone(),
            trust_contract.clone(),
            nft_contract.clone(),
        );

        Self {
            campaign_contract,
            donation_contract,
            trust_contract,
            nft_contract,
            disbursement_contract,
        }
    }

    /// Initialize new user in the system
    pub fn initialize_user(&mut self, user: RistrettoPoint) -> Result<(), String> {
        self.trust_contract.initialize_trust_score(user)
    }

    /// Complete donation flow with all related updates
    pub fn complete_donation_flow(
        &mut self,
        cs: &mut ConstraintSystem,
        campaign_id: [u8; 32],
        donor: RistrettoPoint,
        amount: u64,
        anonymous: bool,
        mint_nft: bool,
    ) -> Result<[u8; 32], String> {
        self.donation_contract.donate(cs, campaign_id, donor, amount, anonymous, mint_nft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

    #[test]
    fn test_campaign_creation() {
        let mut campaign_contract = CampaignContract::new(200); // 2% fee
        let mut cs = ConstraintSystem::new();

        let beneficiary = RISTRETTO_BASEPOINT_POINT;
        let result = campaign_contract.create_campaign(
            &mut cs,
            "Test Campaign".to_string(),
            "A test campaign".to_string(),
            beneficiary,
            10000,
            30,
            "Health".to_string(),
            "Test City".to_string(),
            [0u8; 32],
        );

        assert!(result.is_ok());
        let campaign_id = result.unwrap();
        let campaign = campaign_contract.get_campaign(campaign_id).unwrap();
        assert_eq!(campaign.title, "Test Campaign");
        assert_eq!(campaign.goal_amount, 10000);
    }

    #[test]
    fn test_donation_flow() {
        let mut protocol = SaviaProtocol::new(200);
        let mut cs = ConstraintSystem::new();

        let beneficiary = RISTRETTO_BASEPOINT_POINT;
        let donor = RISTRETTO_BASEPOINT_POINT;

        // Initialize user
        protocol.initialize_user(donor).unwrap();

        // Create campaign
        let campaign_id = protocol.campaign_contract.create_campaign(
            &mut cs,
            "Test Campaign".to_string(),
            "A test campaign".to_string(),
            beneficiary,
            10000,
            30,
            "Health".to_string(),
            "Test City".to_string(),
            [0u8; 32],
        ).unwrap();

        // Make donation
        let donation_id = protocol.complete_donation_flow(
            &mut cs,
            campaign_id,
            donor,
            1000,
            false,
            true,
        ).unwrap();

        // Verify donation
        let donation = protocol.donation_contract.get_donation(donation_id).unwrap();
        assert_eq!(donation.amount, 980); // 1000 - 2% fee
        assert_eq!(donation.nft_minted, true);
    }
}
